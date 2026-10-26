//! Registro das sessões de projeto e o Core separado dos shells sem projeto.
use crate::agents::cli::Exes;
use crate::agents::session::Places;
use crate::config::{self, Choice, Options};
use crate::core::capacity::{Global, Resources};
use crate::core::{Core, Emit};
use crate::proc;
use serde::Serialize;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const UNKNOWN: &str = "Este projeto não está aberto. Abra a pasta novamente.";

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Opened {
    pub id: String,
    pub project: String,
    pub subfolder: bool,
    pub existing: bool,
}

pub type ProjectSnapshot = crate::core::CoreSnapshot;

struct Entry {
    opened: Opened,
    core: Core,
}

#[derive(Default)]
struct Registry {
    next: u64,
    entries: Vec<Entry>,
}

/// Estado compartilhado pelo app: configuração e capacidade globais, estado de execução por projeto.
pub struct Projects {
    registry: Mutex<Registry>,
    exes: Exes,
    places: Places,
    cfg_path: PathBuf,
    emit: Emit,
    capacity: Arc<Global>,
    config_lock: Arc<Mutex<()>>,
    executor_choice: Mutex<Option<Choice>>,
    no_project: Core,
}

impl Projects {
    pub fn new(exes: Exes, places: Places, cfg_path: PathBuf, emit: Emit) -> Projects {
        Self::with_resources(exes, places, cfg_path, emit, Resources::real())
    }

    pub fn with_resources(exes: Exes, places: Places, cfg_path: PathBuf, emit: Emit, resources: Resources) -> Projects {
        let capacity = Global::new(resources);
        let config_lock = Arc::new(Mutex::new(()));
        let no_project = Core::with_capacity(
            exes.clone(), places.clone(), cfg_path.clone(), scoped_emit(emit.clone(), None), capacity.clone(),
            "no-project".into(), config_lock.clone(),
        );
        Projects {
            registry: Mutex::new(Registry::default()),
            exes,
            places,
            cfg_path,
            emit,
            capacity,
            config_lock,
            executor_choice: Mutex::new(None),
            no_project,
        }
    }

    pub fn open_project(&self, folder: &Path) -> Result<Opened, String> {
        let resolved = Core::resolve_project(folder)?;
        if let Some(opened) = self.find_path(&resolved.project) {
            let mut opened = opened;
            opened.existing = true;
            return Ok(opened);
        }
        let id = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            registry.next += 1;
            format!("project-{}", registry.next)
        };
        let core = Core::with_capacity(
            self.exes.clone(), self.places.clone(), self.cfg_path.clone(), scoped_emit(self.emit.clone(), Some(&id)),
            self.capacity.clone(), id.clone(), self.config_lock.clone(),
        );
        let opened = core.set_project(folder)?;
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = registry.entries.iter().find(|entry| {
            proc::mesma_pasta(Path::new(&entry.opened.project), Path::new(&opened.project))
        }) {
            let mut existing = entry.opened.clone();
            existing.existing = true;
            core.shutdown();
            return Ok(existing);
        }
        let result = Opened { id, project: opened.project, subfolder: opened.subfolder, existing: false };
        registry.entries.push(Entry { opened: result.clone(), core: core.clone() });
        if let Some(choice) = self.executor_choice.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            core.apply_executor_choice(choice)?;
        }
        Ok(result)
    }

    fn find_path(&self, project: &str) -> Option<Opened> {
        self.registry.lock().unwrap_or_else(|e| e.into_inner()).entries.iter()
            .find(|entry| proc::mesma_pasta(Path::new(&entry.opened.project), Path::new(project)))
            .map(|entry| entry.opened.clone())
    }

    pub fn close_project(&self, id: &str) -> Result<(), String> {
        let core = {
            let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
            let Some(index) = registry.entries.iter().position(|entry| entry.opened.id == id) else {
                return Err(UNKNOWN.into());
            };
            registry.entries.remove(index).core
        };
        core.shutdown();
        Ok(())
    }

    pub fn core(&self, id: &str) -> Result<Core, String> {
        self.registry.lock().unwrap_or_else(|e| e.into_inner()).entries.iter()
            .find(|entry| entry.opened.id == id).map(|entry| entry.core.clone()).ok_or_else(|| UNKNOWN.into())
    }

    pub fn shell_core(&self, project: Option<&str>) -> Result<Core, String> {
        match project {
            Some(id) => self.core(id),
            None => Ok(self.no_project.clone()),
        }
    }

    pub fn snapshot(&self, id: &str) -> Result<ProjectSnapshot, String> {
        self.core(id)?.snapshot()
    }

    pub fn options(&self) -> Result<Options, String> {
        let cfg = {
            let _config = self.config_lock.lock().unwrap_or_else(|e| e.into_inner());
            config::read(&self.cfg_path)?
        };
        Ok(config::options(&cfg, &self.exes.codex))
    }

    pub fn set_executor_choice(&self, choice: Choice) -> Result<(), String> {
        config::check_choice("executor", &choice)?;
        {
            let _config = self.config_lock.lock().unwrap_or_else(|e| e.into_inner());
            let mut cfg = config::read(&self.cfg_path)?;
            cfg.executor = choice.clone();
            config::save(&self.cfg_path, &cfg)?;
        }
        *self.executor_choice.lock().unwrap_or_else(|e| e.into_inner()) = Some(choice.clone());
        let cores: Vec<Core> = self.registry.lock().unwrap_or_else(|e| e.into_inner()).entries.iter()
            .map(|entry| entry.core.clone()).collect();
        for core in cores {
            let _ = core.apply_executor_choice(choice.clone());
        }
        Ok(())
    }

    pub fn has_running(&self) -> bool {
        let cores: Vec<Core> = self.registry.lock().unwrap_or_else(|e| e.into_inner()).entries.iter()
            .map(|entry| entry.core.clone()).collect();
        cores.into_iter().any(|core| core.has_running().unwrap_or(true))
    }

    pub fn shutdown(&self) {
        let cores: Vec<Core> = self.registry.lock().unwrap_or_else(|e| e.into_inner()).entries.drain(..)
            .map(|entry| entry.core).collect();
        for core in cores { core.shutdown(); }
        self.no_project.shutdown();
    }

    #[cfg(test)]
    pub fn test_emit(&self, id: &str) -> Result<(), String> {
        self.core(id)?.test_emit();
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn test_set_state(&self, id: &str, orchestrator: bool, running: usize, queued: usize) {
        self.core(id).unwrap().test_set_state(orchestrator, running, queued);
    }

    #[cfg(test)]
    pub(crate) fn test_poison(&self, id: &str) {
        self.core(id).unwrap().test_poison_lock();
    }
}

fn scoped_emit(emit: Emit, project: Option<&str>) -> Emit {
    let project = project.map(str::to_string);
    Arc::new(move |event, mut payload| {
        if let Value::Object(object) = &mut payload {
            object.insert("project".into(), project.clone().map_or(Value::Null, Value::String));
        }
        emit(event, payload);
    })
}

impl Drop for Projects {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::capacity::Memory;
    use crate::testutil::{git, identity, write, Tmp};
    use std::sync::mpsc;

    fn fixture(tag: &str) -> (Projects, Tmp, PathBuf, mpsc::Receiver<(String, Value)>) {
        let root = Tmp::new("orq-projects", tag);
        let a = root.0.join("a");
        let b = root.0.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        git(&a, &["init", "-q", "-b", "main"]);
        git(&b, &["init", "-q", "-b", "main"]);
        identity(&a);
        identity(&b);
        write(&a, "a.txt", "a");
        write(&b, "b.txt", "b");
        let (tx, rx) = mpsc::channel();
        let emit: Emit = Arc::new(move |event, payload| drop(tx.send((event.to_string(), payload))));
        let fake = "fake".to_string();
        let exes = Exes { claude: fake.clone(), codex: fake.clone(), app: fake };
        let places = Places { temp: root.0.join("temp"), codex_rules: root.0.join("rules") };
        let resources = Resources {
            cpus: Some(4),
            memory: Arc::new(|| Some(Memory { total: 64 << 30, available: 64 << 30 })),
        };
        let projects = Projects::with_resources(exes, places, root.0.join("config.json"), emit, resources);
        (projects, root, a, rx)
    }

    #[test]
    fn abre_dois_projetos_e_a_mesma_pasta_retorna_o_id_existente() {
        let (projects, root, a_path, _) = fixture("duas-aberturas");
        let b_path = root.0.join("b");
        let a = projects.open_project(&a_path).unwrap();
        let b = projects.open_project(&b_path).unwrap();
        let again = projects.open_project(&a_path.join("." )).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(again.id, a.id);
        assert!(again.existing);
        assert!(projects.core(&a.id).is_ok() && projects.core(&b.id).is_ok());
        assert!(projects.snapshot(&a.id).unwrap().executors_running == 0);
        assert!(projects.snapshot(&b.id).unwrap().executors_running == 0);
        projects.shutdown();
    }

    #[test]
    fn snapshot_eventos_e_falha_ficam_no_projeto_certo() {
        let (projects, root, a_path, rx) = fixture("isolamento");
        let a = projects.open_project(&a_path).unwrap();
        let b = projects.open_project(&root.0.join("b")).unwrap();
        projects.test_set_state(&a.id, true, 2, 3);
        assert_eq!(
            projects.snapshot(&a.id).unwrap(),
            ProjectSnapshot { orchestrator_open: true, executors_running: 2, executors_queued: 3 }
        );
        assert_eq!(projects.snapshot(&b.id).unwrap(), ProjectSnapshot::default());
        projects.test_emit(&a.id).unwrap();
        let (event, payload) = rx.recv().unwrap();
        assert_eq!(event, "test-event");
        assert_eq!(payload["project"], a.id);
        projects.test_poison(&a.id);
        assert_eq!(projects.snapshot(&a.id).unwrap_err(), crate::core::INTERNAL_ERROR);
        assert_eq!(projects.core(&a.id).unwrap().repo().unwrap_err(), crate::core::INTERNAL_ERROR);
        assert!(projects.snapshot(&b.id).is_ok());
        projects.shutdown();
    }

    #[test]
    fn fechar_um_projeto_nao_afeta_o_outro_nem_os_shells_sem_projeto() {
        let (projects, root, a_path, _) = fixture("fechar-um");
        let a = projects.open_project(&a_path).unwrap();
        let b = projects.open_project(&root.0.join("b")).unwrap();
        projects.test_set_state(&a.id, true, 0, 0);
        projects.close_project(&a.id).unwrap();
        assert!(projects.core(&a.id).is_err());
        assert!(projects.core(&b.id).unwrap().repo().is_ok());
        assert!(projects.shell_core(None).unwrap().repo().is_err());
        projects.close_project(&b.id).unwrap();
        assert!(projects.shell_core(None).unwrap().repo().is_err());
    }

    #[test]
    fn escolha_do_executor_invalida_nao_grava_e_valida_persiste() {
        let (projects, root, _, _) = fixture("escolha-executor");
        let config_path = root.0.join("config.json");

        assert!(projects.set_executor_choice(Choice::new("codex", "", "enorme")).is_err());
        assert!(!config_path.exists());

        projects.set_executor_choice(Choice::new("codex", "gpt-5.5", "high")).unwrap();
        let saved: Value = serde_json::from_str(&std::fs::read_to_string(&config_path).unwrap()).unwrap();
        assert_eq!(saved["executor"]["tool"], "codex");
        assert_eq!(saved["executor"]["model"], "gpt-5.5");
        assert_eq!(saved["executor"]["effort"], "high");
    }

}
