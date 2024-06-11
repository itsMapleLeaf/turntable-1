use std::sync::Arc;

use crossbeam::atomic::AtomicCell;
use parking_lot::Mutex;

use crate::{Config, Sink, SinkId};

/// The timeline keeps track of a sequence of sinks, manages advancement of playback, and returns what sinks to preload.
#[derive(Debug, Default)]
pub struct Timeline {
    config: Config,
    /// A sequence of sinks. The first one is the currently playing one.
    sinks: Mutex<Vec<Arc<Sink>>>,
    /// The playback offset of the first sink.
    offset: AtomicCell<usize>,
    /// The total playback offset of the timeline.
    total_offset: AtomicCell<usize>,
}

impl Timeline {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            sinks: Default::default(),
            offset: Default::default(),
            total_offset: Default::default(),
        }
    }

    /// Sets the sinks to play and preload.
    ///
    /// Calling this function will not reset the playback offset to 0 if the first sink is different from the current one.
    /// This is because the player may have already started reading from the next sink, and we don't want to reset the offset.
    ///
    /// Instead, this function is meant to be called when we advance to the next sink, so that future sinks in a queue can be preloaded.
    pub fn set_sinks(&self, sinks: Vec<Arc<Sink>>) {
        let mut current_sinks = self.sinks.lock();

        // Deactivate all sinks that are no longer in the new list.
        for sink in current_sinks.iter() {
            sink.deactivate();
        }

        // Activate all sinks that are in the new list.
        for sink in sinks.iter() {
            sink.activate();
        }

        *current_sinks = sinks;
    }

    /// Advances the playback offset and returns the sinks that the player should read from.
    /// If this returns more than one [TimelineRead], it means the current sink finished playing.
    ///
    /// However, this cannot be relied upon, because it is possible for the remaining samples to be 0 exactly when moving on to the next sink.
    /// In that case the vector will still only be one item, since it breaks before pushing the next [TimelineRead].
    /// Therefore, the player should instead check if the [SinkId] changed to determine if the next sink started playing.
    ///
    /// If the returned vector is empty, it means the player is at the end of the timeline.
    ///
    /// * `amount` - The requested amount of samples.
    pub fn advance(&self, amount: usize) -> Vec<TimelineRead> {
        let mut result = vec![];

        let playable_sinks: Vec<_> = self
            .sinks
            .lock()
            .iter()
            .filter(|s| s.is_playable())
            .cloned()
            .collect();

        let mut remaining = amount;
        let mut playback_offset = self.offset.load();

        for sink in playable_sinks {
            // We've satisified the amount of samples the player wants to play
            if remaining == 0 {
                break;
            }

            let available_until_void = sink.distance_from_void(playback_offset);
            let amount_to_read = available_until_void.distance.min(remaining);
            let new_offset = playback_offset + amount_to_read;

            // There are samples to read from this sink.
            if amount_to_read > 0 {
                remaining -= amount_to_read;
                result.push(TimelineRead {
                    offset: playback_offset,
                    sink: sink.clone(),
                    amount: amount_to_read,
                });

                self.total_offset.fetch_add(amount_to_read);
                self.offset.store(new_offset);
            }

            // Reset offset for the next sink.
            playback_offset = 0;

            // Let's break down the conditions for moving on to the next sink.
            // 1. We've reached the end of the last loaded range of samples, and
            // 2. The sink is sealed/not loadable, meaning there won't be any more samples to load, and
            // 3. There are no more remaining samples to read.
            let should_move_on = !sink.is_loadable()
                && available_until_void.is_end
                && available_until_void.distance.saturating_sub(amount_to_read) == 0;

            // Stop here if we're not moving on to the next sink.
            if !should_move_on {
                break;
            }

            // Otherwise, remove the sink from the list and mark it as consumed.
            sink.deactivate();
            self.offset.store(0);
            self.sinks.lock().retain(|s| s.id != sink.id);
        }

        result
    }

    /// Returns what sink to preload, if any.
    ///
    /// This should never be called while a sink is being loaded.
    pub fn preload(&self) -> Vec<TimelinePreload> {
        let sinks: Vec<_> = self.sinks.lock().iter().cloned().collect();

        let threshold = self.config.preload_threshold_in_samples();

        let mut remaining_to_load = threshold;
        let mut offset = self.offset.load();
        let mut result = vec![];

        for sink in sinks {
            let available_until_void = sink.distance_from_void(offset);
            let available_until_end = sink.distance_from_end(offset);

            // No need to preload if we're under the threshold, or if we satisfied the remaining to load.
            if available_until_void.distance >= threshold || remaining_to_load == 0 {
                break;
            }

            // Only try to preload if the sink is loadable.
            if sink.is_loadable() {
                let how_much_can_preload = available_until_end.min(remaining_to_load);
                let preload_offset = available_until_void.distance + offset;

                result.push(TimelinePreload {
                    sink_id: sink.id,
                    offset: preload_offset,
                });

                remaining_to_load -= how_much_can_preload;
            }

            // Set the preload offset to the start for the next sink.
            offset = 0;
        }

        result
    }

    /// Clears samples that are not needed, to save memory.
    pub fn clear_superflous(&self) {
        let first_sink = self.sinks.lock();
        let offset = self.offset.load();

        if let Some(first_sink) = first_sink.first() {
            first_sink.clear_outside(
                offset,
                self.config.preload_size_in_samples() * 4,
                self.config.channel_count,
            );
        }
    }

    /// Resets the current sink to the beginning.
    pub fn reset(&self) {
        self.offset.store(0);
    }

    /// Returns the offset of the current sink.
    pub fn current_offset(&self) -> usize {
        self.offset.load()
    }

    /// Returns the total offset of the timeline.
    pub fn total_offset(&self) -> usize {
        self.total_offset.load()
    }

    /// Returns true if the timeline is empty.
    pub fn is_empty(&self) -> bool {
        self.sinks.lock().is_empty()
    }
}

/// Instructs a [Player] what sink to read from, and where to start reading from.
#[derive(Debug)]
pub struct TimelineRead {
    pub sink: Arc<Sink>,
    /// The offset of the first sample to read from the sink.
    pub offset: usize,
    /// How many samples to read from the offset.
    pub amount: usize,
}

/// Instructs [Playback] what sinks to preload.
#[derive(Debug)]
pub struct TimelinePreload {
    pub sink_id: SinkId,
    // The offset in samples to start preloading from.
    pub offset: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SinkState;

    #[test]
    fn test_advancement() {
        let config = Config::default();
        let timeline = Timeline::new(config);

        // Set up our sinks.
        let first = Arc::new(Sink::new(Some(10)));
        let second = Arc::new(Sink::new(Some(10)));
        timeline.set_sinks(vec![first.clone(), second.clone()]);

        // First is fully loaded.
        first.write(0, &[1., 2., 3., 4., 5., 6., 7., 8., 9., 10.]);
        first.set_state(SinkState::Sealed);

        // Second has a gap after first range.
        second.write(0, &[1., 2., 3., 4., 5.]);

        // Request 5 samples from the timeline.
        let reads = timeline.advance(5);

        assert_eq!(reads.len(), 1, "only one sink needs to be read");
        assert_eq!(reads[0].offset, 0, "we are at the start of the first sink");
        assert_eq!(reads[0].amount, 5, "five samples should be read");

        // Request 4 samples from the timeline.
        // Next offset should be 5 since we requested 5 samples prior.
        let reads = timeline.advance(4);
        assert_eq!(reads.len(), 1, "only one sink needs to be read");
        assert_eq!(reads[0].offset, 5, "we are at offset 5 of the first sink");

        // Request 5 samples from the timeline.
        // Next offset should be 9 since we requested a total of 9 samples from the first sink.
        // The second sink's offset should start at 0 since we're now at the beginning of the second sink,
        // Due to the remaining samples in the first sink being only 1, meaning we need 4 more samples from the second sink.
        let reads = timeline.advance(5);
        assert_eq!(reads.len(), 2, "returns reads for both sinks");
        assert_eq!(reads[0].offset, 9, "offset for first is correct");
        assert_eq!(reads[1].offset, 0, "next should start at the beginning");

        // We requested 5 samples total, and we had one sample left in the first sink.
        // Therefore, the second sink should be read for the remaining 4 samples.
        assert_eq!(reads[1].amount, 4, "samples requested is correct");

        // Swallow the last sample from the second sink.
        timeline.advance(1);

        // Should return no reads since we're at the end of the timeline.
        let read = timeline.advance(5);
        assert_eq!(read.len(), 0, "no reads should be returned");
    }

    #[test]
    fn test_preload() {
        let config = Config {
            // Makes the threshold amount in samples 3.
            sample_rate: 1,
            channel_count: 1,
            preload_threshold_in_seconds: 3.,
            ..Default::default()
        };

        let timeline = Timeline::new(config);

        // Set up our sinks.
        let first = Arc::new(Sink::new(Some(10)));
        let second = Arc::new(Sink::new(Some(10)));
        timeline.set_sinks(vec![first.clone(), second.clone()]);

        // Should return the first sink to preload.
        let preload = timeline.preload();
        assert_eq!(preload[0].sink_id, first.id, "returns the first sink");

        // We have 3 samples ahead, so we shouldn't need to preload anything.
        first.write(0, &[0., 0., 0.]);
        let preload = timeline.preload();
        assert!(preload.is_empty());

        // Advance by 2 samples.
        timeline.offset.store(2);

        // We don't have any samples, and the offset is now 2, so we need to preload again.
        let preload = timeline.preload();
        assert_eq!(preload[0].offset, 2, "returns the correct offset");

        // Seal the first sink, so we have to preload the second sink.
        first.set_state(SinkState::Sealed);

        // Should return the second sink to preload.
        let preload = timeline.preload();
        assert_eq!(preload[0].sink_id, second.id, "returns the second sink");

        // Set up case where two should be preloaded at once, since they are below the threshold.
        let first = Arc::new(Sink::new(Some(2)));
        let second = Arc::new(Sink::new(Some(2)));

        timeline.reset();
        timeline.set_sinks(vec![first.clone(), second.clone()]);

        let preload = timeline.preload();
        assert_eq!(preload.len(), 2, "returns two preloads");
    }
}
