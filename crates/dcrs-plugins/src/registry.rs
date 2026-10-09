//! The plugin trait, the host that drives it, and lifecycle management.

use std::collections::BTreeMap;

use dcrs_compat::{Registry, Verdict};

use crate::UiNode;
use crate::manifest::{Manifest, RenderSlot};

/// A validated plugin identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PluginId(String);

impl PluginId {
    /// Wraps an id without validating. Use [`Plugin::manifest`] + validation instead.
    #[must_use]
    pub fn new_unchecked(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as a string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PluginId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle and dispatch failures.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// The manifest failed validation.
    #[error("invalid manifest for {id}: {source}")]
    InvalidManifest {
        /// The offending plugin id.
        id: String,
        /// The underlying validation error.
        #[source]
        source: crate::manifest::ManifestError,
    },
    /// The plugin requires a surface the client cannot provide.
    #[error("plugin {id} requires unavailable surface {surface} ({verdict})")]
    UnavailableSurface {
        /// The plugin id.
        id: String,
        /// The surface it needs.
        surface: String,
        /// Why it is unavailable.
        verdict: Verdict,
    },
    /// Another plugin is a dependency and is not enabled.
    #[error("plugin {id} depends on {dependency}, which is not enabled")]
    MissingDependency {
        /// The plugin id.
        id: String,
        /// The unsatisfied dependency.
        dependency: String,
    },
    /// `start` or `stop` returned an error.
    #[error("plugin {id} failed during {phase}: {message}")]
    Failed {
        /// The plugin id.
        id: String,
        /// Which phase failed.
        phase: Phase,
        /// The plugin's message.
        message: String,
    },
    /// A duplicate plugin id was registered.
    #[error("plugin {0} is already registered")]
    Duplicate(String),
    /// No such plugin.
    #[error("no plugin named {0}")]
    NotFound(String),
}

/// Which lifecycle phase a failure occurred in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// `start`.
    Start,
    /// `stop`.
    Stop,
}

impl std::fmt::Display for Phase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Start => "start",
            Self::Stop => "stop",
        })
    }
}

/// A ported plugin.
///
/// Implementors are `'static` so the host can hold them in a heterogeneous collection.
pub trait Plugin: Send + Sync {
    /// Static description.
    fn manifest(&self) -> &Manifest;

    /// Called once when the plugin is enabled.
    fn start(&self, _host: &mut dyn Host) -> Result<(), String> {
        Ok(())
    }

    /// Called once when the plugin is disabled. Must undo everything `start` did.
    fn stop(&self, _host: &mut dyn Host) -> Result<(), String> {
        Ok(())
    }

    /// Produces UI for a slot.
    fn render(&self, _slot: RenderSlot, _host: &mut dyn Host) -> Vec<UiNode> {
        Vec::new()
    }
}

/// What a plugin is allowed to do to the client.
///
/// Deliberately narrow: no filesystem, no network, no process spawning. Anything a plugin needs,
/// the host exposes as a typed method, which is what keeps ports auditable.
pub trait Host {
    /// Reads a settings value for this plugin.
    fn setting(&self, key: &str) -> Option<String>;

    /// Writes a settings value for this plugin.
    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String>;

    /// Shows a transient notification.
    fn notify(&mut self, message: &str);

    /// Logs a line at info level.
    fn log(&mut self, message: &str);
}

/// Where a plugin is in its lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Registered but never started.
    Loaded,
    /// Started and running.
    Running,
    /// Stopped after having run.
    Stopped,
}

/// A registered plugin plus its runtime state.
pub struct LoadedPlugin {
    plugin: Box<dyn Plugin>,
    state: State,
}

impl std::fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("id", &self.plugin.manifest().id)
            .field("state", &self.state)
            .finish()
    }
}

/// A collection of plugins with dependency-ordered lifecycle management.
///
/// Settings are stored here rather than inside [`LoadedPlugin`], because a plugin's `start` runs
/// while its entry is checked out of the map, and it must still be able to write its own settings.
#[derive(Debug, Default)]
pub struct PluginHost {
    plugins: BTreeMap<PluginId, LoadedPlugin>,
    settings: BTreeMap<PluginId, BTreeMap<String, String>>,
    registry: Option<Registry>,
}

impl PluginHost {
    /// Creates an empty host.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Attaches a capability registry, used to reject plugins needing unavailable surfaces.
    #[must_use]
    pub fn with_registry(mut self, registry: Registry) -> Self {
        self.registry = Some(registry);
        self
    }

    /// Registers a plugin without starting it.
    ///
    /// # Errors
    /// Returns an error if the manifest is invalid, a required surface is unavailable, or the id
    /// is already taken.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        let manifest = plugin.manifest();
        let id = manifest.id.clone();

        manifest
            .validate()
            .map_err(|source| PluginError::InvalidManifest {
                id: id.clone(),
                source,
            })?;

        if let Some(registry) = &self.registry {
            for surface in &manifest.required_surfaces {
                let verdict = registry.verdict(surface);
                if !verdict.is_usable() {
                    return Err(PluginError::UnavailableSurface {
                        id,
                        surface: surface.clone(),
                        verdict,
                    });
                }
            }
        }

        let plugin_id = PluginId::new_unchecked(id);
        if self.plugins.contains_key(&plugin_id) {
            return Err(PluginError::Duplicate(plugin_id.to_string()));
        }

        self.plugins.insert(
            plugin_id.clone(),
            LoadedPlugin {
                plugin,
                state: State::Loaded,
            },
        );
        self.settings.entry(plugin_id).or_default();
        Ok(())
    }

    /// Starts a plugin, first starting any of its dependencies that are not yet running.
    ///
    /// # Errors
    /// Returns an error if the plugin is unknown, a dependency cannot be started, or `start`
    /// fails.
    pub fn start(&mut self, id: &str) -> Result<(), PluginError> {
        self.start_inner(id, &mut Vec::new())
    }

    fn start_inner(&mut self, id: &str, visiting: &mut Vec<String>) -> Result<(), PluginError> {
        let plugin_id = PluginId::new_unchecked(id);
        let Some(entry) = self.plugins.get(&plugin_id) else {
            return Err(PluginError::NotFound(id.to_owned()));
        };
        if entry.state == State::Running {
            return Ok(());
        }

        // Guard against indirect dependency cycles rather than recursing forever. Direct
        // self-dependency is already rejected by manifest validation.
        if visiting.iter().any(|v| v == id) {
            return Err(PluginError::MissingDependency {
                id: id.to_owned(),
                dependency: id.to_owned(),
            });
        }
        visiting.push(id.to_owned());

        let deps = entry.plugin.manifest().dependencies.clone();
        for dep in deps {
            if let Some(dep_entry) = self.plugins.get(&PluginId::new_unchecked(&dep)) {
                if dep_entry.state != State::Running {
                    self.start_inner(&dep, visiting)?;
                }
            }
        }
        visiting.pop();

        // Take the entry out so the plugin can hold `&mut self` while its host adapter borrows
        // the host. It is put back below on both success and failure.
        let mut loaded = self
            .plugins
            .remove(&plugin_id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        let result = {
            let mut adapter = HostAdapter {
                plugin_id: id.to_owned(),
                host: self,
            };
            loaded.plugin.start(&mut adapter)
        };
        loaded.state = match result {
            Ok(()) => State::Running,
            Err(message) => {
                self.plugins.insert(plugin_id, loaded);
                return Err(PluginError::Failed {
                    id: id.to_owned(),
                    phase: Phase::Start,
                    message,
                });
            }
        };
        self.plugins.insert(plugin_id, loaded);
        Ok(())
    }

    /// Stops a plugin.
    ///
    /// # Errors
    /// Returns an error if the plugin is unknown or `stop` fails.
    pub fn stop(&mut self, id: &str) -> Result<(), PluginError> {
        let plugin_id = PluginId::new_unchecked(id);
        let mut loaded = self
            .plugins
            .remove(&plugin_id)
            .ok_or_else(|| PluginError::NotFound(id.to_owned()))?;
        let result = {
            let mut adapter = HostAdapter {
                plugin_id: id.to_owned(),
                host: self,
            };
            loaded.plugin.stop(&mut adapter)
        };
        loaded.state = match result {
            Ok(()) => State::Stopped,
            Err(message) => {
                self.plugins.insert(plugin_id, loaded);
                return Err(PluginError::Failed {
                    id: id.to_owned(),
                    phase: Phase::Stop,
                    message,
                });
            }
        };
        self.plugins.insert(plugin_id, loaded);
        Ok(())
    }

    /// Collects rendered nodes for a slot from every running plugin that declares it.
    #[must_use]
    pub fn render_slot(&self, slot: RenderSlot) -> Vec<UiNode> {
        let mut out = Vec::new();
        for entry in self.plugins.values() {
            if entry.state != State::Running {
                continue;
            }
            if !entry.plugin.manifest().render_slots.contains(&slot) {
                continue;
            }
            out.extend(entry.plugin.render(slot, &mut NullHost));
        }
        out
    }

    /// Reads a plugin's setting.
    #[must_use]
    pub fn setting(&self, id: &str, key: &str) -> Option<&str> {
        self.settings
            .get(&PluginId::new_unchecked(id))?
            .get(key)
            .map(String::as_str)
    }

    /// Writes a plugin's setting.
    ///
    /// # Errors
    /// Returns an error if the plugin is not registered.
    pub fn set_setting(&mut self, id: &str, key: &str, value: &str) -> Result<(), String> {
        let plugin_id = PluginId::new_unchecked(id);
        let store = self
            .settings
            .get_mut(&plugin_id)
            .ok_or_else(|| format!("no plugin named {id}"))?;
        store.insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    /// All settings for a plugin.
    #[must_use]
    pub fn settings_of(&self, id: &str) -> Option<&BTreeMap<String, String>> {
        self.settings.get(&PluginId::new_unchecked(id))
    }

    /// Current state of a plugin.
    #[must_use]
    pub fn state(&self, id: &str) -> Option<State> {
        self.plugins
            .get(&PluginId::new_unchecked(id))
            .map(|p| p.state)
    }

    /// Number of registered plugins.
    #[must_use]
    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    /// Whether any plugins are registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }

    /// Ids of all registered plugins.
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plugins.keys().map(PluginId::as_str)
    }
}

/// Bridges a plugin's `Host` calls onto the [`PluginHost`], scoped to one plugin's settings.
///
/// Notifications and log lines are dropped here: the porting tools exercise plugins without a UI,
/// so this adapter is deliberately silent. A real host swaps in a sink.
struct HostAdapter<'a> {
    plugin_id: String,
    host: &'a mut PluginHost,
}

impl Host for HostAdapter<'_> {
    fn setting(&self, key: &str) -> Option<String> {
        self.host.setting(&self.plugin_id, key).map(str::to_owned)
    }

    fn set_setting(&mut self, key: &str, value: &str) -> Result<(), String> {
        let id = self.plugin_id.clone();
        self.host.set_setting(&id, key, value)
    }

    fn notify(&mut self, _message: &str) {}
    fn log(&mut self, _message: &str) {}
}

/// A host that discards everything, used when a plugin renders outside a lifecycle call.
struct NullHost;

impl Host for NullHost {
    fn setting(&self, _key: &str) -> Option<String> {
        None
    }
    fn set_setting(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        Ok(())
    }
    fn notify(&mut self, _message: &str) {}
    fn log(&mut self, _message: &str) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Author, OptionSpec, PluginTag, StartAt};

    fn manifest(id: &str, deps: &[&str]) -> Manifest {
        Manifest {
            id: id.to_owned(),
            name: format!("Plugin {id}"),
            description: "test".to_owned(),
            authors: vec![Author::new("t", 1)],
            search_terms: vec![],
            tags: vec![PluginTag::Utility],
            dependencies: deps.iter().map(|d| (*d).to_owned()).collect(),
            required: false,
            hidden: false,
            enabled_by_default: false,
            requires_restart: false,
            start_at: StartAt::Init,
            render_slots: vec![],
            options: vec![OptionSpec::boolean("x")],
            required_surfaces: vec![],
        }
    }

    /// Records lifecycle calls so tests can assert on ordering.
    #[derive(Clone, Default)]
    struct Recorder {
        log: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }

    struct TestPlugin {
        manifest: Manifest,
        rec: Recorder,
        nodes: Vec<UiNode>,
    }

    impl TestPlugin {
        fn new(id: &str, deps: &[&str]) -> Self {
            Self {
                manifest: manifest(id, deps),
                rec: Recorder::default(),
                nodes: vec![UiNode::text(id)],
            }
        }
        fn with_recorder(mut self, rec: Recorder) -> Self {
            self.rec = rec;
            self
        }
        fn with_nodes(mut self, nodes: Vec<UiNode>) -> Self {
            self.nodes = nodes;
            self
        }
    }

    impl Plugin for TestPlugin {
        fn manifest(&self) -> &Manifest {
            &self.manifest
        }
        fn start(&self, _host: &mut dyn Host) -> Result<(), String> {
            self.rec
                .log
                .lock()
                .unwrap()
                .push(format!("start:{}", self.manifest.id));
            Ok(())
        }
        fn stop(&self, _host: &mut dyn Host) -> Result<(), String> {
            self.rec
                .log
                .lock()
                .unwrap()
                .push(format!("stop:{}", self.manifest.id));
            Ok(())
        }
        fn render(&self, _slot: RenderSlot, _host: &mut dyn Host) -> Vec<UiNode> {
            self.nodes.clone()
        }
    }

    #[test]
    fn register_and_start() {
        let mut host = PluginHost::new();
        host.register(Box::new(TestPlugin::new("a", &[]))).unwrap();
        assert_eq!(host.state("a"), Some(State::Loaded));
        host.start("a").unwrap();
        assert_eq!(host.state("a"), Some(State::Running));
        assert_eq!(host.len(), 1);
        assert_eq!(host.ids().collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn rejects_invalid_manifest() {
        let mut host = PluginHost::new();
        let mut p = TestPlugin::new("bad", &[]);
        p.manifest.id = "Bad Id".to_owned();
        assert!(matches!(
            host.register(Box::new(p)).unwrap_err(),
            PluginError::InvalidManifest { .. }
        ));
    }

    #[test]
    fn rejects_duplicate_ids() {
        let mut host = PluginHost::new();
        host.register(Box::new(TestPlugin::new("a", &[]))).unwrap();
        assert!(matches!(
            host.register(Box::new(TestPlugin::new("a", &[])))
                .unwrap_err(),
            PluginError::Duplicate(_)
        ));
    }

    #[test]
    fn rejects_plugin_needing_unavailable_surface() {
        let registry = Registry::from_surfaces(vec![dcrs_compat::Surface {
            id: "internals.webpackPatches".to_owned(),
            class: dcrs_compat::Class::Internals,
            support: dcrs_compat::Support::Unsupported,
            webapp_analogue: None,
            since: None,
            notes: None,
        }])
        .unwrap();

        let mut p = TestPlugin::new("a", &[]);
        p.manifest.required_surfaces = vec!["internals.webpackPatches".to_owned()];

        let mut host = PluginHost::new().with_registry(registry);
        let err = host.register(Box::new(p)).unwrap_err();
        assert!(matches!(
            err,
            PluginError::UnavailableSurface {
                verdict: Verdict::NotReplicable,
                ..
            }
        ));
    }

    #[test]
    fn dependencies_start_first() {
        let rec = Recorder::default();
        let mut host = PluginHost::new();
        host.register(Box::new(
            TestPlugin::new("base", &[]).with_recorder(rec.clone()),
        ))
        .unwrap();
        host.register(Box::new(
            TestPlugin::new("dep", &["base"]).with_recorder(rec.clone()),
        ))
        .unwrap();

        host.start("dep").unwrap();

        let log = rec.log.lock().unwrap().clone();
        let base_idx = log.iter().position(|l| l == "start:base").unwrap();
        let dep_idx = log.iter().position(|l| l == "start:dep").unwrap();
        assert!(
            base_idx < dep_idx,
            "dependency must start first, got {log:?}"
        );
    }

    #[test]
    fn starting_twice_is_a_no_op() {
        let rec = Recorder::default();
        let mut host = PluginHost::new();
        host.register(Box::new(
            TestPlugin::new("a", &[]).with_recorder(rec.clone()),
        ))
        .unwrap();
        host.start("a").unwrap();
        host.start("a").unwrap();
        assert_eq!(rec.log.lock().unwrap().len(), 1);
    }

    #[test]
    fn dependency_cycles_do_not_hang() {
        let mut host = PluginHost::new();
        host.register(Box::new(TestPlugin::new("a", &["b"])))
            .unwrap();
        host.register(Box::new(TestPlugin::new("b", &["a"])))
            .unwrap();
        // Manifest validation catches the direct self-dependency case; this guards the indirect one.
        assert!(host.start("a").is_err());
    }

    #[test]
    fn unknown_plugin_start_is_an_error() {
        let mut host = PluginHost::new();
        assert!(matches!(
            host.start("nope").unwrap_err(),
            PluginError::NotFound(_)
        ));
        assert!(matches!(
            host.stop("nope").unwrap_err(),
            PluginError::NotFound(_)
        ));
    }

    /// Leaks a manifest so a plugin can return a `&'static` reference to it. Only for tests.
    fn leaked(id: &str, deps: &[&str]) -> &'static Manifest {
        Box::leak(Box::new(manifest(id, deps)))
    }

    #[test]
    fn settings_round_trip_through_host_adapter() {
        struct Writer;
        impl Plugin for Writer {
            fn manifest(&self) -> &Manifest {
                leaked("w", &[])
            }
            fn start(&self, host: &mut dyn Host) -> Result<(), String> {
                assert_eq!(host.setting("k"), None);
                host.set_setting("k", "v")?;
                assert_eq!(host.setting("k").as_deref(), Some("v"));
                host.notify("hello");
                host.log("world");
                Ok(())
            }
        }
        let mut host = PluginHost::new();
        host.register(Box::new(Writer)).unwrap();
        host.start("w").unwrap();
        assert_eq!(host.setting("w", "k"), Some("v"));
    }

    #[test]
    fn only_declaring_running_plugins_contribute_to_slots() {
        let mut host = PluginHost::new();
        let mut a = TestPlugin::new("a", &[]).with_nodes(vec![UiNode::button("from-a")]);
        a.manifest.render_slots = vec![RenderSlot::ChatBarButton];
        let mut b = TestPlugin::new("b", &[]).with_nodes(vec![UiNode::button("from-b")]);
        b.manifest.render_slots = vec![RenderSlot::ChatBarButton];
        // c declares the slot but is never started.
        let mut c = TestPlugin::new("c", &[]).with_nodes(vec![UiNode::button("from-c")]);
        c.manifest.render_slots = vec![RenderSlot::ChatBarButton];
        // d runs but declares no slots.
        let d = TestPlugin::new("d", &[]).with_nodes(vec![UiNode::button("from-d")]);

        host.register(Box::new(a)).unwrap();
        host.register(Box::new(b)).unwrap();
        host.register(Box::new(c)).unwrap();
        host.register(Box::new(d)).unwrap();
        host.start("a").unwrap();
        host.start("b").unwrap();
        host.start("d").unwrap();

        let nodes = host.render_slot(RenderSlot::ChatBarButton);
        let texts: Vec<_> = nodes.iter().filter_map(|n| n.text.as_deref()).collect();
        assert_eq!(texts, vec!["from-a", "from-b"]);
    }

    #[test]
    fn stopping_transitions_state() {
        let mut host = PluginHost::new();
        host.register(Box::new(TestPlugin::new("a", &[]))).unwrap();
        host.start("a").unwrap();
        host.stop("a").unwrap();
        assert_eq!(host.state("a"), Some(State::Stopped));
        assert_eq!(host.render_slot(RenderSlot::ChatBarButton), Vec::new());
    }

    #[test]
    fn start_failure_reports_phase() {
        struct Failing;
        impl Plugin for Failing {
            fn manifest(&self) -> &Manifest {
                leaked("f", &[])
            }
            fn start(&self, _host: &mut dyn Host) -> Result<(), String> {
                Err("boom".to_owned())
            }
        }
        let mut host = PluginHost::new();
        host.register(Box::new(Failing)).unwrap();
        let err = host.start("f").unwrap_err();
        match err {
            PluginError::Failed { phase, message, .. } => {
                assert_eq!(phase, Phase::Start);
                assert_eq!(message, "boom");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn ui_nodes_construct_and_style() {
        let n = UiNode::text("hi").class("vnc-plugins-foo");
        assert_eq!(n.kind, "text");
        assert_eq!(n.text.as_deref(), Some("hi"));
        assert_eq!(n.classes, vec!["vnc-plugins-foo".to_owned()]);
        assert_eq!(UiNode::spacer().text, None);
        assert_eq!(UiNode::button("ok").kind, "button");
    }
}
