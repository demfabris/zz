use super::{DispatchNode, DispatchNodeId, DispatchTree};
use crate::{EntityId, FocusId};
use std::{mem, ops::Range};

pub(super) fn copy_nodes(
    target: &mut DispatchTree,
    source: &mut DispatchTree,
    range: Range<usize>,
    focus: Option<FocusId>,
) -> bool {
    let start = target.nodes.len();
    let base_parent = target.node_stack.last().copied();
    let mut contains_focus = false;
    target.nodes.reserve(range.len());
    for index in range.clone() {
        let node = &mut source.nodes[index];
        let parent = match node.parent {
            Some(parent) if range.contains(&parent.0) => {
                Some(DispatchNodeId(parent.0 - range.start + start))
            }
            _ => base_parent,
        };
        let id = DispatchNodeId(target.nodes.len());
        if node.focus_id.is_some() && node.focus_id == focus {
            contains_focus = true;
        }
        if let Some(focus_id) = node.focus_id {
            target.focusable_node_ids.insert(focus_id, id);
        }
        let view_id = node
            .view_id
            .filter(|&view_id| innermost_view(target, parent) != Some(view_id));
        if let Some(view_id) = view_id {
            target.view_node_ids.insert(view_id, id);
        }
        target.nodes.push(DispatchNode {
            key_listeners: mem::take(&mut node.key_listeners),
            action_listeners: mem::take(&mut node.action_listeners),
            modifiers_changed_listeners: mem::take(&mut node.modifiers_changed_listeners),
            context: node.context.clone(),
            focus_id: node.focus_id,
            view_id,
            parent,
        });
    }
    contains_focus
}

fn innermost_view(target: &DispatchTree, mut node: Option<DispatchNodeId>) -> Option<EntityId> {
    while let Some(id) = node {
        let node_ref = &target.nodes[id.0];
        if node_ref.view_id.is_some() {
            return node_ref.view_id;
        }
        node = node_ref.parent;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActionRegistry, KeyContext, Keymap};
    use std::{any::TypeId, cell::RefCell, rc::Rc};

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        fn one_in(&mut self, n: usize) -> bool {
            self.below(n) == 0
        }
    }

    fn tree() -> DispatchTree {
        DispatchTree::new(
            Rc::new(RefCell::new(Keymap::new(Vec::new()))),
            Rc::new(ActionRegistry::default()),
        )
    }

    fn focus_id(n: u64) -> FocusId {
        slotmap::KeyData::from_ffi(n).into()
    }

    fn decorate(tree: &mut DispatchTree, rng: &mut Rng) {
        if rng.one_in(4) {
            let name = ["a", "b", "c"][rng.below(3)];
            tree.set_key_context(KeyContext::parse(name).unwrap());
        }
        if rng.one_in(5) {
            tree.set_focus_id(focus_id(1 + rng.below(40) as u64));
        }
        if rng.one_in(4) {
            tree.set_view_id(EntityId::from(1 + rng.below(3) as u64));
        }
        for _ in 0..rng.below(3) {
            tree.on_action(TypeId::of::<u8>(), Rc::new(|_, _, _, _| {}));
        }
        if rng.one_in(4) {
            tree.on_key_event(Rc::new(|_, _, _, _| {}));
        }
        if rng.one_in(5) {
            tree.on_modifiers_changed(Rc::new(|_, _, _| {}));
        }
    }

    fn source(seed: u64, len: usize) -> DispatchTree {
        let mut rng = Rng(seed);
        let mut tree = tree();
        let mut depth = 0;
        while tree.len() < len {
            tree.push_node();
            depth += 1;
            decorate(&mut tree, &mut rng);
            let closes = if rng.one_in(3) {
                rng.below(depth + 1)
            } else {
                0
            };
            for _ in 0..closes {
                tree.pop_node();
                depth -= 1;
            }
        }
        for _ in 0..depth {
            tree.pop_node();
        }
        tree
    }

    fn target(seed: u64) -> DispatchTree {
        let mut rng = Rng(seed);
        let mut tree = tree();
        for _ in 0..1 + rng.below(4) {
            tree.push_node();
            decorate(&mut tree, &mut rng);
        }
        tree
    }

    fn copy_one_by_one(
        target: &mut DispatchTree,
        source: &mut DispatchTree,
        range: Range<usize>,
        focus: Option<FocusId>,
    ) -> bool {
        let mut open = Vec::new();
        let mut contains_focus = false;
        for index in range {
            let node = &mut source.nodes[index];
            while let Some(&last) = open.last() {
                if node.parent == Some(last) {
                    break;
                }
                open.pop();
                target.pop_node();
            }
            open.push(DispatchNodeId(index));
            if node.focus_id.is_some() && node.focus_id == focus {
                contains_focus = true;
            }
            target.move_node(node);
        }
        while open.pop().is_some() {
            target.pop_node();
        }
        contains_focus
    }

    fn assert_same(expected: &DispatchTree, actual: &DispatchTree, seed: u64) {
        assert_eq!(expected.nodes.len(), actual.nodes.len(), "seed {seed}");
        for (expected, actual) in expected.nodes.iter().zip(&actual.nodes) {
            assert_eq!(expected.parent, actual.parent, "seed {seed}");
            assert_eq!(expected.context, actual.context, "seed {seed}");
            assert_eq!(expected.focus_id, actual.focus_id, "seed {seed}");
            assert_eq!(expected.view_id, actual.view_id, "seed {seed}");
            assert_eq!(
                expected.key_listeners.len(),
                actual.key_listeners.len(),
                "seed {seed}"
            );
            assert_eq!(
                expected.action_listeners.len(),
                actual.action_listeners.len(),
                "seed {seed}"
            );
            assert_eq!(
                expected.modifiers_changed_listeners.len(),
                actual.modifiers_changed_listeners.len(),
                "seed {seed}"
            );
        }
        assert_eq!(expected.node_stack, actual.node_stack, "seed {seed}");
        assert_eq!(expected.context_stack, actual.context_stack, "seed {seed}");
        assert_eq!(expected.view_stack, actual.view_stack, "seed {seed}");
        assert_eq!(
            expected.focusable_node_ids, actual.focusable_node_ids,
            "seed {seed}"
        );
        assert_eq!(expected.view_node_ids, actual.view_node_ids, "seed {seed}");
    }

    #[test]
    fn bulk_copy_matches_upstream_node_copy_for_closed_ranges() {
        for seed in 1..400u64 {
            let len = 5 + (seed as usize * 7) % 120;
            let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
            let start = rng.below(len + 1);
            let range = start..start + rng.below(len - start + 1);
            let focus = Some(focus_id(1 + rng.below(40) as u64));
            let mut expected_source = source(seed, len);
            let mut actual_source = source(seed, len);
            let mut expected = target(seed);
            let mut actual = target(seed);
            let expected_focus =
                copy_one_by_one(&mut expected, &mut expected_source, range.clone(), focus);
            let actual_focus = copy_nodes(&mut actual, &mut actual_source, range.clone(), focus);
            assert_eq!(expected_focus, actual_focus, "seed {seed}");
            assert_same(&expected, &actual, seed);
            assert_same(&expected_source, &actual_source, seed);
        }
    }
}
