mod cpal_output;

use std::sync::Arc;

use crate::playback::{AudioError, RealtimeControl};
use crate::source::PlaybackSource;

pub(crate) use cpal_output::CpalThreadOutput;

/// Platform output boundary. Implementations open a live stream on the control
/// thread and hand only prebuilt state plus atomics to the callback.
pub(crate) trait OutputBackend {
    fn ensure_started(
        &mut self,
        control: Arc<RealtimeControl>,
        source: PlaybackSource,
    ) -> Result<(), AudioError>;
}
