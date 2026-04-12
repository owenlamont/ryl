pub(crate) struct Walker<T, M = ()> {
    containers: Vec<ContainerState<T, M>>,
    key_depth: usize,
}

impl<T, M> Walker<T, M> {
    pub(crate) const fn new() -> Self {
        Self {
            containers: Vec::new(),
            key_depth: 0,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.containers.clear();
        self.key_depth = 0;
    }

    pub(crate) fn enter_mapping(&mut self, mapping: T, metadata: M) {
        let context = self.begin_node();
        self.containers.push(ContainerState {
            key_context: context.active,
            metadata,
            mapping: Some(MappingState {
                expect_key: true,
                payload: mapping,
            }),
        });
    }

    pub(crate) fn enter_sequence(&mut self, metadata: M) {
        let context = self.begin_node();
        self.containers.push(ContainerState {
            key_context: context.active,
            metadata,
            mapping: None,
        });
    }

    pub(crate) fn exit_container(&mut self) {
        if let Some(container) = self.containers.pop()
            && container.key_context
            && self.key_depth > 0
        {
            self.key_depth -= 1;
        }
    }

    pub(crate) fn begin_node(&mut self) -> NodeContext {
        let mut key_root = false;
        if let Some(ContainerState {
            mapping: Some(mapping),
            ..
        }) = self.containers.last_mut()
        {
            if mapping.expect_key {
                key_root = true;
                mapping.expect_key = false;
            } else {
                mapping.expect_key = true;
            }
        }
        let active = key_root || self.key_depth > 0;
        if active {
            self.key_depth += 1;
        }
        NodeContext { active, key_root }
    }

    pub(crate) const fn finish_node(&mut self, context: NodeContext) {
        if context.active && self.key_depth > 0 {
            self.key_depth -= 1;
        }
    }

    pub(crate) fn current_mapping_mut(&mut self) -> Option<&mut T> {
        self.containers
            .last_mut()
            .and_then(|container| container.mapping.as_mut())
            .map(|mapping| &mut mapping.payload)
    }

    pub(crate) fn any_metadata(&self, mut predicate: impl FnMut(&M) -> bool) -> bool {
        self.containers
            .iter()
            .any(|container| predicate(&container.metadata))
    }
}

struct ContainerState<T, M> {
    key_context: bool,
    metadata: M,
    mapping: Option<MappingState<T>>,
}

struct MappingState<T> {
    expect_key: bool,
    payload: T,
}

#[derive(Copy, Clone)]
pub(crate) struct NodeContext {
    active: bool,
    key_root: bool,
}

impl NodeContext {
    pub(crate) const fn active(self) -> bool {
        self.active
    }

    pub(crate) const fn key_root(self) -> bool {
        self.key_root
    }
}
