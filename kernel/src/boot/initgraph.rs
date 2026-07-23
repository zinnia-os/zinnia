use super::BootInfo;
use crate::util::mutex::spin::{SpinMutex, SpinMutexGuard};
use alloc::string::String;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use intrusive_collections::{LinkedList, LinkedListLink, intrusive_adapter};

intrusive_adapter!(pub InLinkAdapter = &'static Edge: Edge { in_link => LinkedListLink });
intrusive_adapter!(pub OutLinkAdapter = &'static Edge: Edge { out_link => LinkedListLink });
intrusive_adapter!(pub PendingLinkAdapter = &'static Node: Node { pending_link => LinkedListLink });

pub struct Edge {
    source: &'static Node,
    target: &'static Node,

    propagates_disable: bool,

    in_link: LinkedListLink,
    out_link: LinkedListLink,
}

unsafe impl Sync for Edge {}

impl Edge {
    pub const fn new(
        source: &'static Node,
        target: &'static Node,
        propagates_disable: bool,
    ) -> Self {
        Self {
            source,
            target,

            propagates_disable,

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
    Gate(fn() -> bool),
}

pub struct Node {
    display_name: &'static str,

    unsatisfied_deps: AtomicUsize,
    wanted: AtomicBool,
    done: AtomicBool,
    disabled: AtomicBool,

    in_edges: SpinMutex<LinkedList<InLinkAdapter>>,
    out_edges: SpinMutex<LinkedList<OutLinkAdapter>>,
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
            disabled: AtomicBool::new(false),

            in_edges: SpinMutex::new(LinkedList::new(InLinkAdapter::NEW)),
            out_edges: SpinMutex::new(LinkedList::new(OutLinkAdapter::NEW)),
            pending_link: LinkedListLink::new(),
        }
    }

    pub fn display_name(&self) -> &'static str {
        self.display_name
    }

    pub fn is_disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    pub fn in_edges(&self) -> SpinMutexGuard<'_, LinkedList<InLinkAdapter>> {
        self.in_edges.lock()
    }

    pub fn out_edges(&self) -> SpinMutexGuard<'_, LinkedList<OutLinkAdapter>> {
        self.out_edges.lock()
    }

    #[doc(hidden)]
    pub fn on_reached(&self) {
        assert!(self.wanted.load(Ordering::Relaxed));
        assert!(!self.done.load(Ordering::Relaxed));
        assert_eq!(self.unsatisfied_deps.load(Ordering::Relaxed), 0);

        if !self.disabled.load(Ordering::Relaxed) {
            match self.action {
                Action::Empty => {}
                Action::Callback(func) => func(),
                Action::Gate(func) => {
                    if !func() {
                        self.disabled.store(true, Ordering::Relaxed);
                    }
                }
            }
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

fn get_all_nodes() -> &'static [Node] {
    let nodes_start = &raw const LD_INIT_START as *const Node;
    let nodes_end = &raw const LD_INIT_END as *const Node;

    unsafe { core::slice::from_raw_parts(nodes_start, nodes_end.offset_from_unsigned(nodes_start)) }
}

/// # Safety
/// This function must be called exactly once.
unsafe fn initialize_edges() {
    let ctors_start = &raw const LD_INIT_CTORS_START as *const fn();
    let ctors_end = &raw const LD_INIT_CTORS_END as *const fn();

    for ctor in unsafe {
        core::slice::from_raw_parts(ctors_start, ctors_end.offset_from_unsigned(ctors_start))
    } {
        ctor();
    }
}

fn execute_graph(goal: Option<&'static Node>, mut on_node_reached: impl FnMut(&Node)) {
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

        let disabled = node.disabled.load(Ordering::Relaxed);

        for edge in node.out_edges.lock().iter() {
            let successor = edge.target;

            assert_ne!(successor.unsatisfied_deps.load(Ordering::Relaxed), 0);

            successor.unsatisfied_deps.fetch_sub(1, Ordering::Relaxed);

            if disabled && edge.propagates_disable {
                successor.disabled.store(true, Ordering::Relaxed);
            }

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

/// Runs the global initialization sequence.
pub fn run() {
    unsafe {
        initialize_edges();
    }

    execute_graph(None, |node| {
        if node.is_disabled() {
            status!("Skipping stage \"{}\"", node.display_name());
        } else {
            status!("Running stage \"{}\"", node.display_name());
        }
    });

    status!("All stages are complete!");

    if BootInfo::get()
        .command_line
        .get_bool("initgraph")
        .unwrap_or(false)
    {
        let mut graph = String::new();

        graph += "digraph initgraph {\n";
        graph += "\tsubgraph {\n";

        for node in get_all_nodes() {
            graph += &format!("\t\tn{:p} [label={:?}];\n", node, node.display_name());

            for edge in node.in_edges().iter() {
                graph += &format!("\t\t\tn{:p} -> n{:p};\n", edge.source(), edge.target());
            }
        }

        graph += "\t}\n}";

        log!("{}", graph);
    }
}
