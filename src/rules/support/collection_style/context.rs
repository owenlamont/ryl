use crate::rules::support::mapping_key_walker::Walker;
use ryl_yaml_parser::StructureStyle;

#[derive(Clone, Copy)]
pub(crate) struct RetainedContext(bool);

impl RetainedContext {
    pub(crate) fn from_style(style: StructureStyle) -> Self {
        Self(style == StructureStyle::Flow)
    }

    pub(crate) fn enclosing<T>(walker: &Walker<T, Self>) -> Self {
        Self(walker.any_metadata(|context| context.0))
    }

    pub(crate) const fn in_flow(self) -> bool {
        self.0
    }
}
