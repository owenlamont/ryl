use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel::{Sender, bounded};
use url::Url;

use crate::config::{ConfigContext, Env};
use crate::rules::schema::PreparationError;
use crate::rules::schema::directives::Revision;
use crate::rules::schema::net::{self, Dependencies, Network, Policy};
use crate::yaml_dom::YamlOwned;

pub(super) type Origins = BTreeMap<String, BTreeSet<String>>;

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub preparation: Duration,
    pub retained_bytes: usize,
    pub retained_resources: usize,
    pub document_selections: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            preparation: Duration::from_secs(65),
            retained_bytes: crate::rules::schema::MAX_SCHEMA_BYTES,
            retained_resources: crate::rules::schema::MAX_SCHEMA_RESOURCES,
            document_selections: crate::rules::schema::MAX_SCHEMA_RESOURCES,
        }
    }
}

enum Status {
    Queued,
    Running,
    AwaitingApproval(PreparationError),
    Finished(Result<Arc<ConfigContext>, PreparationError>),
}

struct Entry {
    group: Option<String>,
    context: ConfigContext,
    status: Status,
    deadline: Instant,
    cancel: Arc<AtomicBool>,
    source_text: String,
    origins: Origins,
    dependencies: BTreeSet<PathBuf>,
}

impl Entry {
    fn superseded_by(&self, context: &ConfigContext, group: Option<&str>) -> bool {
        self.context.source == context.source
            && self.context.base_dir == context.base_dir
            && (self.group.as_deref() != group
                || self
                    .context
                    .config
                    .schema_request()
                    .map(|request| &request.owner)
                    == context
                        .config
                        .schema_request()
                        .map(|request| &request.owner))
    }
}

#[derive(Default)]
struct State {
    entries: BTreeMap<String, Entry>,
    queue: VecDeque<String>,
    grants: BTreeSet<String>,
    persist: VecDeque<String>,
}

impl State {
    fn prune_queue(&mut self) {
        self.queue.retain(|key| self.entries.contains_key(key));
    }
}

pub(super) struct Service {
    state: Arc<Mutex<State>>,
    wake: Sender<()>,
    worker: Option<JoinHandle<()>>,
    pub(super) env: Arc<dyn Env + Send + Sync>,
    pub(super) limits: Limits,
}

impl Service {
    pub(super) fn new(
        env: Arc<dyn Env + Send + Sync>,
        limits: Limits,
        events: Sender<super::Event>,
    ) -> Self {
        let state = Arc::new(Mutex::new(State::default()));
        let (wake, jobs) = bounded(1);
        let owned = state.clone();
        let environment = env.clone();
        let worker = std::thread::spawn(move || {
            run_service(&owned, &jobs, environment.as_ref(), &events, limits);
        });
        Self {
            state,
            wake,
            worker: Some(worker),
            env,
            limits,
        }
    }

    pub(super) fn request(
        &self,
        context: ConfigContext,
    ) -> Result<ConfigContext, PreparationError> {
        let key = format!("{}:{:?}", context.base_dir.display(), context.config);
        let group = context.config.schema_directives().then(|| {
            format!(
                "{}:{}",
                context.base_dir.display(),
                context.config.to_toml_string()
            )
        });
        let mut state = self.state.lock().expect("schema service lock");
        if let Some(entry) = state.entries.get_mut(&key) {
            if Instant::now() >= entry.deadline
                && matches!(entry.status, Status::Queued | Status::Running)
            {
                entry.cancel.store(true, Ordering::Relaxed);
                entry.status = Status::Finished(Err(preparation_error(
                    &entry.context,
                    "schema preparation deadline exceeded".to_owned(),
                )));
            }
            return match &entry.status {
                Status::Finished(result) => result
                    .as_ref()
                    .map(|context| context.as_ref().clone())
                    .map_err(Clone::clone),
                Status::AwaitingApproval(error) => Err(PreparationError {
                    message: format!(
                        "schema origin approval required: {}",
                        entry.origins.keys().cloned().collect::<Vec<_>>().join(" ")
                    ),
                    ..error.clone()
                }),
                Status::Queued | Status::Running => Err(preparation_error(
                    &entry.context,
                    "schema remote preparation pending".to_owned(),
                )),
            };
        }
        let obsolete: Vec<_> = state
            .entries
            .iter()
            .filter(|(_, entry)| entry.superseded_by(&context, group.as_deref()))
            .map(|(key, _)| key.clone())
            .collect();
        for key in obsolete {
            state
                .entries
                .remove(&key)
                .expect("existing entry")
                .cancel
                .store(true, Ordering::Relaxed);
            state.queue.retain(|queued| queued != &key);
        }
        if group.is_some()
            && state
                .entries
                .values()
                .filter(|entry| entry.group == group)
                .count()
                >= self.limits.document_selections
        {
            return Err(preparation_error(&context, net::RESOURCE_LIMIT.to_owned()));
        }
        let pending =
            preparation_error(&context, "schema remote preparation pending".to_owned());
        state.entries.insert(
            key.clone(),
            Entry {
                group,
                context,
                status: Status::Queued,
                deadline: Instant::now() + self.limits.preparation,
                cancel: Arc::new(AtomicBool::new(false)),
                source_text: String::new(),
                origins: Origins::new(),
                dependencies: BTreeSet::new(),
            },
        );
        state.queue.push_back(key);
        let _ = self.wake.try_send(());
        Err(pending)
    }

    fn associations(&self) -> Vec<(PathBuf, Origins, String)> {
        self.state
            .lock()
            .expect("schema service lock")
            .entries
            .values()
            .filter_map(|entry| {
                if matches!(entry.status, Status::AwaitingApproval(..))
                    && entry.context.config.schema_request().is_none()
                {
                    entry.context.source.clone().map(|source| {
                        (source, entry.origins.clone(), entry.source_text.clone())
                    })
                } else {
                    None
                }
            })
            .collect()
    }

    pub(super) fn needed(&self) -> Origins {
        let state = self.state.lock().expect("schema service lock");
        let mut needed = Origins::new();
        for entry in state.entries.values() {
            if !matches!(entry.status, Status::Finished(_)) {
                for (origin, urls) in &entry.origins {
                    needed
                        .entry(origin.clone())
                        .or_default()
                        .extend(urls.iter().cloned());
                }
            }
        }
        needed
    }

    fn persist(&self, origin: String) {
        let mut state = self.state.lock().expect("schema service lock");
        state.persist.push_back(origin);
        let _ = self.wake.try_send(());
    }

    pub(super) fn allow(&self, origin: &str) {
        let mut state = self.state.lock().expect("schema service lock");
        state.grants.insert(origin.to_owned());
        let mut queued = Vec::new();
        for (key, entry) in &mut state.entries {
            if matches!(entry.status, Status::AwaitingApproval(..))
                && entry.origins.contains_key(origin)
                && !entry.cancel.load(Ordering::Relaxed)
            {
                entry.status = Status::Queued;
                entry.deadline = Instant::now() + self.limits.preparation;
                queued.push(key.clone());
            }
        }
        state.queue.extend(queued);
        let _ = self.wake.try_send(());
    }

    pub(super) fn cancel_pending(&self) {
        let mut state = self.state.lock().expect("schema service lock");
        state.entries.retain(|_, entry| {
            if entry.context.config.schema_request().is_some()
                || matches!(entry.status, Status::Queued | Status::Running)
            {
                entry.cancel.store(true, Ordering::Relaxed);
                false
            } else {
                true
            }
        });
        state.queue.clear();
    }

    pub(super) fn invalidate(&self) {
        let mut state = self.state.lock().expect("schema service lock");
        for entry in state.entries.values() {
            entry.cancel.store(true, Ordering::Relaxed);
        }
        state.entries.clear();
        state.queue.clear();
    }

    pub(super) fn shutdown(&self) {
        self.invalidate();
        let _ = self.wake.try_send(());
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        self.invalidate();
        let (dummy, _) = bounded(1);
        let old = std::mem::replace(&mut self.wake, dummy);
        drop(old);
        let worker = self.worker.take().expect("schema worker is joined once");
        let _ = worker.join();
    }
}

struct Job {
    key: String,
    context: ConfigContext,
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    grants: Vec<String>,
}

fn next_job(owned: &Mutex<State>) -> Option<Job> {
    let mut state = owned.lock().expect("schema service lock");
    let State {
        entries,
        queue,
        grants,
        ..
    } = &mut *state;
    let key = std::iter::from_fn(|| queue.pop_front())
        .find(|key| matches!(entries[key].status, Status::Queued))?;
    let grants = grants.iter().cloned().collect();
    let entry = entries.get_mut(&key).expect("selected entry exists");
    entry.status = Status::Running;
    Some(Job {
        key,
        context: entry.context.clone(),
        cancel: entry.cancel.clone(),
        deadline: entry.deadline,
        grants,
    })
}

fn run_service(
    owned: &Mutex<State>,
    jobs: &crossbeam_channel::Receiver<()>,
    environment: &(dyn Env + Send + Sync),
    events: &Sender<super::Event>,
    limits: Limits,
) {
    while jobs.recv().is_ok() {
        loop {
            let origin = owned
                .lock()
                .expect("schema service lock")
                .persist
                .pop_front();
            if let Some(origin) = origin {
                let result = net::persist_origin(environment, &origin);
                let _ = events.send(super::Event::SchemaGrant { origin, result });
            } else if let Some(job) = next_job(owned) {
                perform_job(job, owned, environment, events, limits);
            } else {
                break;
            }
        }
    }
}

fn perform_job(
    mut job: Job,
    owned: &Mutex<State>,
    environment: &(dyn Env + Send + Sync),
    events: &Sender<super::Event>,
    limits: Limits,
) {
    let policy = Arc::new(Approval {
        cancel: job.cancel.clone(),
        deadline: job.deadline,
        needed: Mutex::default(),
    });
    let env = SchemaEnv {
        base: environment,
        grants: job.grants,
        policy: Some(policy.clone()),
        remote: true,
        lint: true,
        dependencies: std::sync::OnceLock::new(),
    };
    let prepared = prepare(&mut job.context, &env);
    let source_text = job
        .context
        .source
        .as_ref()
        .and_then(|path| environment.read_to_string(path).ok())
        .unwrap_or_default();
    let mut result = prepared.map(|()| Arc::new(job.context));
    let needed =
        std::mem::take(&mut *policy.needed.lock().expect("schema approval lock"));
    let mut state = owned.lock().expect("schema service lock");
    if let Some(current) = state.entries.get(&job.key) {
        let (mut bytes, mut resources) = result
            .as_ref()
            .map_or((0, 0), |context| context.config.schema_usage());
        for (key, entry) in &state.entries {
            if key != &job.key
                && current.group.is_some()
                && entry.group == current.group
                && let Status::Finished(Ok(context)) = &entry.status
            {
                let usage = context.config.schema_usage();
                bytes += usage.0;
                resources += usage.1;
            }
        }
        if bytes > limits.retained_bytes {
            result = Err(preparation_error(
                &current.context,
                net::SIZE_LIMIT.to_owned(),
            ));
        } else if resources > limits.retained_resources {
            result = Err(preparation_error(
                &current.context,
                net::RESOURCE_LIMIT.to_owned(),
            ));
        }
    }
    let expired = job.cancel.load(Ordering::Relaxed) || Instant::now() >= job.deadline;
    let retry = !expired && needed.keys().any(|origin| state.grants.contains(origin));
    let Some(entry) = state.entries.get_mut(&job.key) else {
        return;
    };
    if !Arc::ptr_eq(&entry.cancel, &job.cancel)
        || matches!(entry.status, Status::Finished(_))
    {
        return;
    }
    entry.source_text = source_text;
    entry.dependencies = env.take_dependencies();
    entry.origins = needed;
    entry.status = if expired {
        entry.cancel.store(true, Ordering::Relaxed);
        Status::Finished(Err(preparation_error(
            &entry.context,
            "schema preparation cancelled or deadline exceeded".to_owned(),
        )))
    } else if retry {
        Status::Queued
    } else if entry.origins.is_empty() {
        Status::Finished(result)
    } else {
        Status::AwaitingApproval(
            result.expect_err("unapproved origins prevent preparation"),
        )
    };
    if retry {
        state.queue.push_back(job.key);
    }
    drop(state);
    let _ = events.send(super::Event::Schema);
}

fn preparation_error(context: &ConfigContext, message: String) -> PreparationError {
    match context.config.schema_request() {
        Some(request) => request.error(message),
        None => PreparationError {
            line: 1,
            column: 1,
            message,
        },
    }
}

fn prepare(
    context: &mut ConfigContext,
    env: &SchemaEnv<'_>,
) -> Result<(), PreparationError> {
    let urls: Vec<_> = context
        .config
        .rule_option("schema", "schemas")
        .and_then(YamlOwned::as_sequence)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.as_mapping_get("url").and_then(YamlOwned::as_str))
        .chain(
            context
                .config
                .schema_request()
                .into_iter()
                .flat_map(|request| request.declarations.iter())
                .filter_map(|declaration| declaration.value.as_deref())
                .filter(|value| {
                    value
                        .get(..6)
                        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https:"))
                }),
        )
        .collect();
    if !urls.is_empty() {
        let network = Network::from_env(env, BTreeMap::new())
            .map_err(|error| preparation_error(context, error.to_string()))?;
        let mut failure = None;
        for url in urls {
            if let Err(error) = network.authorize(url) {
                let mut error = preparation_error(context, error.to_string());
                if let Some(declaration) = context
                    .config
                    .schema_request()
                    .and_then(|request| request.declaration_for(url))
                {
                    error.line = declaration.line;
                    error.column = declaration.column;
                }
                failure = Some(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
    }
    context.config.prepare_schemas(&context.base_dir, env)
}

struct Approval {
    cancel: Arc<AtomicBool>,
    deadline: Instant,
    needed: Mutex<Origins>,
}
impl Policy for Approval {
    fn check(&self, url: &Url, granted: bool) -> Result<(), net::Error> {
        if self.cancel.load(Ordering::Relaxed) || Instant::now() >= self.deadline {
            return Err(net::Error::Policy(
                "schema preparation cancelled or deadline exceeded",
            ));
        }
        if !granted {
            self.needed
                .lock()
                .expect("schema approval lock")
                .entry(url.origin().ascii_serialization())
                .or_default()
                .insert(url.to_string());
        }
        Ok(())
    }
}

pub(super) struct SchemaEnv<'a> {
    base: &'a dyn Env,
    grants: Vec<String>,
    policy: Option<Arc<Approval>>,
    remote: bool,
    lint: bool,
    dependencies: std::sync::OnceLock<Arc<Mutex<BTreeSet<PathBuf>>>>,
}
impl<'a> SchemaEnv<'a> {
    pub(super) fn discovery(base: &'a dyn Env, lint: bool) -> Self {
        Self {
            base,
            lint,
            remote: false,
            grants: Vec::new(),
            policy: None,
            dependencies: std::sync::OnceLock::new(),
        }
    }

    fn take_dependencies(&self) -> BTreeSet<PathBuf> {
        self.dependencies.get().map_or_else(BTreeSet::new, |paths| {
            std::mem::take(&mut *paths.lock().expect("schema dependencies lock"))
        })
    }
}
impl Env for SchemaEnv<'_> {
    fn local_schema_dependencies(&self) -> Option<Arc<Mutex<BTreeSet<PathBuf>>>> {
        Some(self.dependencies.get_or_init(Arc::default).clone())
    }
    fn remote_schemas(&self) -> bool {
        self.remote
    }
    fn prepare_lint_schemas(&self) -> bool {
        self.lint
    }
    fn schema_policy(&self) -> Option<Arc<dyn Policy>> {
        Some(
            self.policy
                .as_ref()
                .expect("only workers construct schema networks")
                .clone(),
        )
    }
    fn session_schema_origins(&self) -> Vec<String> {
        self.grants.clone()
    }
    fn schema_dependencies(&self) -> Option<Dependencies> {
        self.base.schema_dependencies()
    }
    fn current_dir(&self) -> PathBuf {
        self.base.current_dir()
    }
    fn config_dir(&self) -> Option<PathBuf> {
        self.base.config_dir()
    }
    fn home_dir(&self) -> Option<PathBuf> {
        self.base.home_dir()
    }
    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        self.base.read_to_string(path)
    }
    fn path_exists(&self, path: &Path) -> bool {
        self.base.path_exists(path)
    }
    fn env_var(&self, key: &str) -> Option<String> {
        self.base.env_var(key)
    }
}

pub(super) struct DocumentDependencies {
    paths: BTreeSet<PathBuf>,
    service_key: Option<String>,
    directives: bool,
}

pub(super) struct Session {
    pub(super) generation: std::sync::atomic::AtomicU64,
    pub(super) env: Arc<dyn Env + Send + Sync>,
    pub(super) limits: Limits,
    pub(super) events: Sender<super::Event>,
    pub(super) service: std::sync::OnceLock<Arc<Service>>,
    pub(super) dependencies:
        std::sync::OnceLock<Mutex<BTreeMap<PathBuf, DocumentDependencies>>>,
    pub(super) revisions: std::sync::OnceLock<Mutex<BTreeMap<PathBuf, Revision>>>,
}
impl Session {
    fn close_revision(&self, uri: &str) {
        let Some(revisions) = self.revisions.get() else {
            return;
        };
        let owner =
            super::encoding::uri_to_path(uri).unwrap_or_else(|| PathBuf::from(uri));
        revisions
            .lock()
            .expect("schema revisions lock")
            .remove(&owner);
        if let Some(service) = self.service.get() {
            let mut state = service.state.lock().expect("schema service lock");
            state.entries.retain(|_, entry| {
                let keep = entry
                    .context
                    .config
                    .schema_request()
                    .is_none_or(|request| request.owner != owner);
                if !keep {
                    entry.cancel.store(true, Ordering::Relaxed);
                }
                keep
            });
            state.prune_queue();
        }
    }

    pub(super) fn update_revision(&self, uri: &str, revision: i32) {
        if let Some(revisions) = self.revisions.get()
            && let Some(current) =
                revisions.lock().expect("schema revisions lock").get_mut(
                    super::encoding::uri_to_path(uri)
                        .as_deref()
                        .unwrap_or_else(|| Path::new(uri)),
                )
        {
            current.version = i64::from(revision);
        }
    }

    pub(super) fn register_revision(
        &self,
        owner: &Path,
        revision: Option<i64>,
        text: &str,
    ) {
        if let Some(revision) = revision {
            self.revisions
                .get_or_init(Mutex::default)
                .lock()
                .expect("schema revisions lock")
                .insert(owner.to_owned(), Revision::new(revision, text));
        }
    }

    pub(super) fn finish_document(
        &self,
        path: PathBuf,
        context: &ConfigContext,
        env: &SchemaEnv<'_>,
        generation: u64,
        result: Result<(), PreparationError>,
    ) -> Result<Option<ConfigContext>, PreparationError> {
        let request = context
            .config
            .schema_request()
            .expect("directive preparation has a request");
        let revisions = self
            .revisions
            .get()
            .map(|revisions| revisions.lock().expect("schema revisions lock"));
        let current = revisions
            .as_ref()
            .and_then(|revisions| revisions.get(&request.owner))
            .copied();
        if current != request.revision {
            return Err(preparation_error(
                context,
                "schema preparation superseded by a newer document revision".to_owned(),
            ));
        }
        self.observe_document(context);
        self.record_dependencies(path, env, generation, Some(context));
        result?;
        if context.config.needs_remote_preparation() {
            self.request_document(context.clone()).map(Some)
        } else {
            Ok(None)
        }
    }
    pub(super) fn observe_document(&self, context: &ConfigContext) {
        let Some(service) = self.service.get() else {
            return;
        };
        let request = context
            .config
            .schema_request()
            .expect("only directive preparations are observed");
        let key = format!("{}:{:?}", context.base_dir.display(), context.config);
        let mut state = service.state.lock().expect("schema service lock");
        state.entries.retain(|old, entry| {
            let obsolete = old != &key
                && entry
                    .context
                    .config
                    .schema_request()
                    .is_some_and(|current| current.owner == request.owner);
            if obsolete {
                entry.cancel.store(true, Ordering::Relaxed);
            }
            !obsolete
        });
        state.prune_queue();
    }

    pub(super) fn record_dependencies(
        &self,
        path: PathBuf,
        env: &SchemaEnv<'_>,
        generation: u64,
        context: Option<&ConfigContext>,
    ) {
        let paths = env.take_dependencies();
        if env.dependencies.get().is_none() {
            if let Some(dependencies) = self.dependencies.get() {
                let mut dependencies =
                    dependencies.lock().expect("schema dependencies lock");
                if self.generation.load(Ordering::Relaxed) == generation {
                    dependencies.remove(&path);
                }
            }
            return;
        }
        let mut dependencies = self
            .dependencies
            .get_or_init(Mutex::default)
            .lock()
            .expect("schema dependencies lock");
        if self.generation.load(Ordering::Relaxed) == generation {
            dependencies.insert(
                path,
                DocumentDependencies {
                    paths,
                    directives: context.is_some_and(|context| {
                        context.config.schema_request().is_some()
                    }),
                    service_key: context
                        .filter(|context| context.config.needs_remote_preparation())
                        .map(|context| {
                            format!(
                                "{}:{:?}",
                                context.base_dir.display(),
                                context.config
                            )
                        }),
                },
            );
        }
    }

    fn dependency_paths(
        &self,
        mut tracked: impl FnMut(&Path) -> bool,
    ) -> BTreeSet<PathBuf> {
        let mut paths = BTreeSet::new();
        let mut service_keys = BTreeSet::new();
        if let Some(dependencies) = self.dependencies.get() {
            let mut dependencies =
                dependencies.lock().expect("schema dependencies lock");
            dependencies.retain(|path, _| tracked(path));
            for dependency in dependencies.values() {
                paths.extend(dependency.paths.iter().cloned());
                service_keys.extend(dependency.service_key.iter().cloned());
            }
        }
        if let Some(service) = self.service.get() {
            let mut state = service.state.lock().expect("schema service lock");
            state.entries.retain(|key, entry| {
                let retain = entry.context.config.schema_request().is_none()
                    || service_keys.contains(key);
                if !retain {
                    entry.cancel.store(true, Ordering::Relaxed);
                }
                retain
            });
            state.prune_queue();
            paths.extend(
                state
                    .entries
                    .iter()
                    .filter(|(key, _)| service_keys.contains(*key))
                    .flat_map(|(_, entry)| entry.dependencies.iter().cloned()),
            );
        }
        paths
    }
    pub(super) fn request(
        &self,
        context: ConfigContext,
    ) -> Result<ConfigContext, String> {
        self.request_document(context)
            .map_err(|error| error.to_string())
    }

    pub(super) fn request_document(
        &self,
        context: ConfigContext,
    ) -> Result<ConfigContext, PreparationError> {
        self.service
            .get_or_init(|| {
                Arc::new(Service::new(
                    self.env.clone(),
                    self.limits,
                    self.events.clone(),
                ))
            })
            .request(context)
    }
}

fn origin_title(prefix: &str, origin: &str) -> String {
    format!(
        "{prefix} {}",
        net::display_url(origin).trim_end_matches('/')
    )
}

impl super::Server {
    fn schema_dependency_paths(&self) -> BTreeSet<PathBuf> {
        self.settings
            .schema
            .as_ref()
            .expect("schema session")
            .dependency_paths(|path| {
                self.settings.reports_workspace()
                    || self
                        .documents
                        .keys()
                        .any(|uri| self.uri_path(uri).0 == path)
            })
    }

    pub(super) fn watched_files_changed(
        &mut self,
        connection: &lsp_server::Connection,
        params: &serde_json::Value,
    ) {
        let paths = self.schema_dependency_paths();
        let is_dependency = |path: &Path| {
            paths.iter().any(|dependency| {
                crate::config::paths_equal(
                    dependency,
                    path,
                    crate::config::CASE_INSENSITIVE_PATHS,
                )
            })
        };
        let changed = super::parse::<lsp_types::DidChangeWatchedFilesParams>(params)
            .is_none_or(|watched| {
                watched.changes.iter().any(|event| {
                    self.is_config_uri(event.uri.as_str())
                        || super::encoding::uri_to_path(event.uri.as_str())
                            .as_deref()
                            .is_some_and(is_dependency)
                })
            });
        if changed {
            self.invalidate_schemas(connection);
            self.handle_config_change(connection);
        }
        self.wake(connection);
    }

    pub(super) fn sync_schema_watchers(&mut self, connection: &lsp_server::Connection) {
        if !self.supports_watch_registration {
            return;
        }
        let paths = self.schema_dependency_paths();
        if paths == self.schema_watches {
            return;
        }
        if !self.schema_watches.is_empty() {
            self.client_request(
                connection,
                "client/unregisterCapability",
                lsp_types::UnregistrationParams {
                    unregisterations: vec![lsp_types::Unregistration {
                        id: "ryl-watch-schemas".to_owned(),
                        method: "workspace/didChangeWatchedFiles".to_owned(),
                    }],
                },
            );
        }
        self.schema_watches = paths;
        if self.schema_watches.is_empty() {
            return;
        }
        let watchers: Vec<_> = self
            .schema_watches
            .iter()
            .map(|path| {
                let literal =
                    globset::escape(&path.to_string_lossy().replace('\\', "/"));
                lsp_types::FileSystemWatcher {
                    glob_pattern: lsp_types::GlobPattern::String(literal),
                    kind: None,
                }
            })
            .collect();
        self.client_request(
            connection,
            "client/registerCapability",
            lsp_types::RegistrationParams {
                registrations: vec![lsp_types::Registration {
                    id: "ryl-watch-schemas".to_owned(),
                    method: "workspace/didChangeWatchedFiles".to_owned(),
                    register_options: Some(serde_json::json!({"watchers": watchers})),
                }],
            },
        );
    }

    pub(super) fn schema_service(&self) -> Option<Arc<Service>> {
        self.settings
            .schema
            .as_ref()
            .expect("server settings always carry a schema session")
            .service
            .get()
            .cloned()
    }

    pub(super) fn cancel_unobserved_schemas(&self) {
        if self.documents.is_empty()
            && self.pull.is_none()
            && let Some(service) = self.schema_service()
        {
            service.cancel_pending();
            self.settings
                .schema
                .as_ref()
                .expect("schema session")
                .dependencies
                .get_or_init(Mutex::default)
                .lock()
                .expect("schema dependencies lock")
                .retain(|_, dependency| !dependency.directives);
        }
    }

    pub(super) fn close_schema_document(&self, uri: &str) {
        let session = self.settings.schema.as_ref().expect("schema session");
        session.close_revision(uri);
        if !self.settings.reports_workspace()
            && let Some(dependencies) = self
                .settings
                .schema
                .as_ref()
                .expect("schema session")
                .dependencies
                .get()
        {
            let (path, _) = self.uri_path(uri);
            dependencies
                .lock()
                .expect("schema dependencies lock")
                .remove(&path);
        }
        self.cancel_unobserved_schemas();
    }

    pub(super) fn invalidate_schemas(&mut self, connection: &lsp_server::Connection) {
        self.settings
            .schema
            .as_ref()
            .expect("schema session")
            .generation
            .fetch_add(1, Ordering::Relaxed);
        if let Some(dependencies) = self
            .settings
            .schema
            .as_ref()
            .expect("schema session")
            .dependencies
            .get()
        {
            dependencies
                .lock()
                .expect("schema dependencies lock")
                .clear();
        }
        for (_, uri) in self.schema_config_diagnostics.drain() {
            super::publish(connection, uri, None, Vec::new());
        }
        if let Some(service) = self.schema_service() {
            service.invalidate();
        }
    }

    pub(super) fn schema_ready(&mut self, connection: &lsp_server::Connection) {
        self.handle_config_change(connection);
        self.publish_schema_associations(connection);
        self.wake(connection);
    }

    fn publish_schema_associations(&mut self, connection: &lsp_server::Connection) {
        let service = self
            .schema_service()
            .expect("schema events have a preparation service");
        let mut old = std::mem::take(&mut self.schema_config_diagnostics);
        for (path, origins, text) in service.associations() {
            let uri = super::encoding::path_to_uri(&path);
            let diagnostics = origins
                .into_iter()
                .map(|(origin, urls)| {
                    let start = urls
                        .iter()
                        .find_map(|url| text.find(url))
                        .or_else(|| text.find("[[lint.rules.schema.schemas]]"))
                        .unwrap_or(0);
                    let prefix: Vec<_> = text[..start].split('\n').collect();
                    let column =
                        prefix.last().expect("split has one line").chars().count() + 1;
                    let lines: Vec<_> = text.split('\n').collect();
                    let position = super::encoding::position_at(
                        &lines,
                        prefix.len(),
                        column,
                        self.encoding,
                    );
                    let mut diagnostic = super::config_error_diagnostic(&format!(
                        "schema origin approval required: {origin}"
                    ));
                    diagnostic.range = lsp_types::Range::new(position, position);
                    diagnostic
                })
                .collect();
            old.remove(uri.as_str());
            self.schema_config_diagnostics
                .insert(uri.as_str().to_owned(), uri.clone());
            super::publish(connection, uri, None, diagnostics);
        }
        for (_, uri) in old {
            super::publish(connection, uri, None, Vec::new());
        }
    }

    pub(super) fn schema_actions(
        &self,
        params: &lsp_types::CodeActionParams,
    ) -> lsp_types::CodeActionResponse {
        if !params.context.diagnostics.iter().any(|diagnostic| {
            diagnostic.source.as_deref() == Some("ryl")
                && diagnostic
                    .message
                    .contains("schema origin approval required")
        }) || !super::actions::admits(params.context.only.as_deref(), "quickfix")
        {
            return Vec::new();
        }
        let Some(service) = self.schema_service() else {
            return Vec::new();
        };
        if net::offline(service.env.as_ref()) {
            return Vec::new();
        }
        service
            .needed()
            .keys()
            .filter(|origin| {
                let message = super::config_error_text(&format!(
                    "schema origin approval required: {origin}"
                ));
                params.context.diagnostics.iter().any(|diagnostic| {
                    diagnostic.source.as_deref() == Some("ryl")
                        && diagnostic.message == message
                        && diagnostic.range.start <= params.range.end
                        && params.range.start <= diagnostic.range.end
                })
            })
            .flat_map(|origin| {
                [true, false].map(move |persist| {
                    let title = origin_title(
                        if persist {
                            "Always allow"
                        } else {
                            "Allow once"
                        },
                        origin,
                    );
                    lsp_types::CodeActionOrCommand::CodeAction(lsp_types::CodeAction {
                        title: title.clone(),
                        kind: Some(lsp_types::CodeActionKind::QUICKFIX),
                        command: Some(lsp_types::Command {
                            title,
                            command: "ryl.allowSchemaOrigin".to_owned(),
                            arguments: Some(vec![
                                serde_json::Value::String(origin.clone()),
                                serde_json::Value::Bool(persist),
                            ]),
                        }),
                        ..Default::default()
                    })
                })
            })
            .collect()
    }

    pub(super) fn schema_command(
        &mut self,
        params: &serde_json::Value,
    ) -> serde_json::Value {
        if params.get("command").and_then(serde_json::Value::as_str)
            != Some("ryl.allowSchemaOrigin")
        {
            return serde_json::Value::Null;
        }
        let Some(origin) = params
            .pointer("/arguments/0")
            .and_then(serde_json::Value::as_str)
        else {
            return serde_json::Value::Null;
        };
        let Some(service) = self.schema_service() else {
            return serde_json::Value::Null;
        };
        if !net::offline(service.env.as_ref()) && service.needed().contains_key(origin)
        {
            if params
                .pointer("/arguments/1")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true)
            {
                service.persist(origin.to_owned());
            } else {
                service.allow(origin);
            }
        }
        serde_json::Value::Null
    }
}
