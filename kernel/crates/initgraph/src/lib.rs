#![no_std]

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use intrusive_collections::{LinkedList, LinkedListLink, intrusive_adapter};
use spin::{Mutex, MutexGuard};

pub use initgraph_proc::*;

intrusive_adapter!(pub InLinkAdapter = &'static Edge: Edge { in_link => LinkedListLink });
intrusive_adapter!(pub OutLinkAdapter = &'static Edge: Edge { out_link => LinkedListLink });
intrusive_adapter!(pub PendingLinkAdapter = &'static Node: Node { pending_link => LinkedListLink });

pub struct Edge {
    source: &'static Node,
    target: &'static Node,

    in_link: LinkedListLink,
    out_link: LinkedListLink,
}

unsafe impl Sync for Edge {}

impl Edge {
    pub const fn new(source: &'static Node, target: &'static Node) -> Self {
        Self {
            source,
            target,

            in_link: LinkedListLink::new(),
            out_link: LinkedListLink::new(),
        }
    }

    pub fn source(&self) -> &'static Node {
        self.source
    }

    pub fn target(&self) -> &'static Node {
        self.target
    }

    #[doc(hidden)]
    pub fn register(&'static self) {
        self.source.out_edges.lock().push_back(self);
        self.target.in_edges.lock().push_back(self);
        self.target.unsatisfied_deps.fetch_add(1, Ordering::Relaxed);
    }
}

pub enum Action {
    Empty,
    Callback(fn()),
}

pub struct Node {
    display_name: &'static str,

    unsatisfied_deps: AtomicUsize,
    wanted: AtomicBool,
    done: AtomicBool,

    in_edges: Mutex<LinkedList<InLinkAdapter>>,
    out_edges: Mutex<LinkedList<OutLinkAdapter>>,
    pending_link: LinkedListLink,

    action: Action,
}

unsafe impl Sync for Node {}

impl Node {
    pub const fn new(display_name: &'static str, action: Action) -> Self {
        Self {
            display_name,
            action,

            unsatisfied_deps: AtomicUsize::new(0),
            wanted: AtomicBool::new(false),
            done: AtomicBool::new(false),

            in_edges: Mutex::new(LinkedList::new(InLinkAdapter::NEW)),
            out_edges: Mutex::new(LinkedList::new(OutLinkAdapter::NEW)),
            pending_link: LinkedListLink::new(),
        }
    }

    pub fn display_name(&self) -> &'static str {
        self.display_name
    }

    pub fn in_edges(&self) -> MutexGuard<'_, LinkedList<InLinkAdapter>> {
        self.in_edges.lock()
    }

    pub fn out_edges(&self) -> MutexGuard<'_, LinkedList<OutLinkAdapter>> {
        self.out_edges.lock()
    }

    #[doc(hidden)]
    pub fn on_reached(&self) {
        assert!(self.wanted.load(Ordering::Relaxed));
        assert!(!self.done.load(Ordering::Relaxed));
        assert_eq!(self.unsatisfied_deps.load(Ordering::Relaxed), 0);

        match self.action {
            Action::Empty => {}
            Action::Callback(func) => func(),
        }

        self.done.store(true, Ordering::Relaxed);
    }
}

unsafe extern "C" {
    static LD_INIT_CTORS_START: u8;
    static LD_INIT_CTORS_END: u8;
    static LD_INIT_START: u8;
    static LD_INIT_END: u8;
}

pub fn get_all_nodes() -> &'static [Node] {
    let nodes_start = &raw const LD_INIT_START as *const Node;
    let nodes_end = &raw const LD_INIT_END as *const Node;

    unsafe { core::slice::from_raw_parts(nodes_start, nodes_end.offset_from_unsigned(nodes_start)) }
}

/// # Safety
/// This function must be called exactly once.
pub unsafe fn initialize_edges() {
    let ctors_start = &raw const LD_INIT_CTORS_START as *const fn();
    let ctors_end = &raw const LD_INIT_CTORS_END as *const fn();

    for ctor in unsafe {
        core::slice::from_raw_parts(ctors_start, ctors_end.offset_from_unsigned(ctors_start))
    } {
        ctor();
    }
}

pub fn execute_graph(goal: Option<&'static Node>, mut on_node_reached: impl FnMut(&Node)) {
    let nodes = get_all_nodes();

    if let Some(goal) = goal {
        let mut queue = LinkedList::new(PendingLinkAdapter::NEW);

        if !goal.wanted.load(Ordering::Relaxed) {
            queue.push_back(goal);
            goal.wanted.store(true, Ordering::Relaxed);
        }

        while let Some(node) = queue.pop_front() {
            for in_edge in node.in_edges.lock().iter() {
                if !in_edge.source.wanted.load(Ordering::Relaxed) {
                    queue.push_back(in_edge.source);
                    in_edge.source.wanted.store(true, Ordering::Relaxed);
                }
            }
        }
    } else {
        for node in nodes {
            node.wanted.store(true, Ordering::Relaxed);
        }
    }

    let mut pending = LinkedList::new(PendingLinkAdapter::NEW);

    for node in nodes.iter().filter(|node| {
        node.wanted.load(Ordering::Relaxed)
            && !node.done.load(Ordering::Relaxed)
            && node.unsatisfied_deps.load(Ordering::Relaxed) == 0
    }) {
        pending.push_back(node);
    }

    while let Some(node) = pending.pop_front() {
        on_node_reached(node);
        node.on_reached();

        for edge in node.out_edges.lock().iter() {
            let successor = edge.target;

            assert_ne!(successor.unsatisfied_deps.load(Ordering::Relaxed), 0);

            successor.unsatisfied_deps.fetch_sub(1, Ordering::Relaxed);

            if successor.wanted.load(Ordering::Relaxed)
                && !successor.done.load(Ordering::Relaxed)
                && successor.unsatisfied_deps.load(Ordering::Relaxed) == 0
            {
                pending.push_back(successor);
            }
        }
    }

    for node in nodes.iter().filter(|x| x.wanted.load(Ordering::Relaxed)) {
        assert!(
            node.done.load(Ordering::Relaxed),
            "The dependencies for node {:?} could not be resolved!",
            node.display_name()
        );
    }
}
