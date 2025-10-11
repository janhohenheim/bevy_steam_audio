use audionimbus::ReflectionEffectParams;
use bevy_seedling::node::RegisterNode as _;
use firewheel::{
    channel_config::ChannelConfig,
    diff::{Diff, Patch, RealtimeClone},
    event::ProcEvents,
    node::{
        AudioNode, AudioNodeInfo, AudioNodeProcessor, ConstructProcessorContext, EmptyConfig,
        ProcBuffers, ProcExtra, ProcInfo, ProcessStatus,
    },
};

use crate::prelude::*;

pub(super) fn plugin(app: &mut App) {
    app.register_node::<ReverbDataNode>();
}

pub(crate) struct SharedReverbData(pub(crate) ReflectionEffectParams);

#[derive(Component, Patch, Diff, Clone, RealtimeClone)]
pub(crate) struct ReverbDataNode;

impl AudioNode for ReverbDataNode {
    type Configuration = EmptyConfig;

    fn info(&self, _: &Self::Configuration) -> AudioNodeInfo {
        AudioNodeInfo::new().channel_config(ChannelConfig::new(0, 0))
    }

    fn construct_processor(
        &self,
        _configuration: &Self::Configuration,
        _cx: ConstructProcessorContext,
    ) -> impl AudioNodeProcessor {
        Self
    }
}

impl AudioNodeProcessor for ReverbDataNode {
    fn process(
        &mut self,
        _info: &ProcInfo,
        _buffers: ProcBuffers,
        events: &mut ProcEvents,
        extra: &mut ProcExtra,
    ) -> ProcessStatus {
        for mut event in events.drain() {
            if let Some(params) = event.downcast::<ReflectionEffectParams>()
                && let Err(params) = extra.store.insert(SharedReverbData(params))
            {
                extra.store.get_mut::<SharedReverbData>().0 = params.0;
            }
        }

        ProcessStatus::ClearAllOutputs
    }
}
