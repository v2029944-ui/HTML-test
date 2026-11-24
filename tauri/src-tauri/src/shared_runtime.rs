//! One checked-out data workspace per desktop process. Network files are transport only.
use super::shared_folder::{self, Checkout, Publication};
use super::{err, Config, Storage, TransferPreview, TransferReceipt};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Access {
    Read,
    Write,
    Local,
    PersonalOnly,
}
#[derive(Clone, Debug, Serialize)]
pub struct WorkspaceStatus {
    pub mode: String,
    pub folder: Option<String>,
    pub generation: Option<u64>,
    pub available: bool,
    pub error: Option<String>,
    pub pending: bool,
    pub workspace_token: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    folder: Option<PathBuf>,
    config: Option<Config>,
    #[serde(default)]
    highest_generation: u64,
    #[serde(default)]
    highest_digest: Option<String>,
}
struct Pending {
    candidate: Option<Checkout>,
    publication: Option<Publication>,
}
struct State {
    personal: Storage,
    selection: Selection,
    checkout: Option<Checkout>,
    pending: Option<Pending>,
    available: bool,
    error: Option<String>,
    token: String,
    grants: Vec<PathBuf>,
}
pub struct Runtime {
    root: PathBuf,
    sessions: Mutex<Arc<Mutex<Option<super::FolderImportSession>>>>,
    state: Mutex<State>,
}
fn token() -> String {
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    let entropy = format!(
        "{}:{:?}:{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH),
        SERIAL.fetch_add(1, Ordering::SeqCst)
    );
    format!("{:x}", Sha256::digest(entropy.as_bytes()))
}
pub(crate) fn check_local_cache(root: &Path) -> Result<(), String> {
    let _canonical = fs::canonicalize(root).map_err(err)?;
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetVolumePathNameW(file: *const u16, volume: *mut u16, len: u32) -> i32;
            fn GetDriveTypeW(root: *const u16) -> u32;
        }
        let path: Vec<u16> = _canonical
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect();
        let mut volume = vec![0u16; 32768];
        if unsafe { GetVolumePathNameW(path.as_ptr(), volume.as_mut_ptr(), volume.len() as u32) }
            == 0
        {
            return Err("Не удалось проверить локальный диск кэша общей базы.".into());
        }
        if !matches!(unsafe { GetDriveTypeW(volume.as_ptr()) }, 2 | 3 | 6) {
            return Err("Кэш общей базы должен находиться на локальном диске компьютера, вне сетевых папок.".into());
        }
    }
    Ok(())
}
impl Runtime {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        Self::with_cache(root.clone(), root.join("private-workspace"))
    }
    pub fn with_cache(personal_root: PathBuf, root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(err)?;
        check_local_cache(&root)?;
        fs::create_dir_all(root.join("shared-cache")).map_err(err)?;
        let personal = Storage::new(personal_root)?;
        let selection = match fs::read(root.join("shared-workspace.json")) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(err)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Selection::default(),
            Err(e) => return Err(err(e)),
        };
        let sessions = Mutex::new(Arc::clone(&personal.folder_import));
        let runtime = Self {
            root,
            sessions,
            state: Mutex::new(State {
                personal,
                selection,
                checkout: None,
                pending: None,
                available: true,
                error: None,
                token: token(),
                grants: Vec::new(),
            }),
        };
        {
            let mut state = runtime.state.lock().map_err(err)?;
            if state.selection.folder.is_some() {
                let pending = runtime.pending_dir(&state)?;
                if pending.exists() {
                    match Publication::resume(state.selection.folder.as_deref().unwrap(), &pending)
                    {
                        Ok(publication) => {
                            state.pending = Some(Pending {
                                candidate: None,
                                publication: Some(publication),
                            })
                        }
                        Err(error) => {
                            state.pending = Some(Pending {
                                candidate: None,
                                publication: None,
                            });
                            state.available = false;
                            state.error = Some(error);
                        }
                    }
                }
                if state.pending.is_some() {
                    state.available = false;
                    if state.error.is_none() {
                        state.error = Some(
                            "Есть неподтверждённая публикация общей базы. Нажмите «Повторить»."
                                .into(),
                        );
                    }
                } else if state.error.is_none() {
                    if let Err(error) = runtime.pull(&mut state) {
                        state.available = false;
                        state.error = Some(error);
                    }
                }
            }
        }
        Ok(runtime)
    }
    fn cache_dir(&self, state: &State) -> Result<PathBuf, String> {
        let folder = state
            .selection
            .folder
            .as_ref()
            .ok_or("Общая папка не выбрана.")?;
        let key = format!("{:x}", Sha256::digest(folder.to_string_lossy().as_bytes()));
        let dir = self.root.join("shared-cache").join(key);
        fs::create_dir_all(&dir).map_err(err)?;
        Ok(dir)
    }
    fn pending_dir(&self, state: &State) -> Result<PathBuf, String> {
        Ok(self.cache_dir(state)?.join("pending"))
    }
    fn save_selection(&self, selection: &Selection) -> Result<(), String> {
        let write = |destination: PathBuf| -> Result<(), String> {
            let parent = destination
                .parent()
                .ok_or("Нет папки локальных настроек.")?;
            fs::create_dir_all(parent).map_err(err)?;
            let mut file = tempfile::NamedTempFile::new_in(parent).map_err(err)?;
            serde_json::to_writer(&mut file, selection).map_err(err)?;
            file.flush().map_err(err)?;
            file.as_file().sync_all().map_err(err)?;
            file.persist(destination)
                .map_err(|failure| err(failure.error))?;
            Ok(())
        };
        if let Some(folder) = &selection.folder {
            let key = format!("{:x}", Sha256::digest(folder.to_string_lossy().as_bytes()));
            write(
                self.root
                    .join("shared-cache")
                    .join(key)
                    .join("workspace.json"),
            )?;
        }
        write(self.root.join("shared-workspace.json"))
    }
    fn check_generation(selection: &Selection, generation: u64) -> Result<(), String> {
        if generation < selection.highest_generation {
            Err(format!("Общая папка потеряла подтверждённое поколение {} (найдено {}). Восстановите общую папку; пустая база не создана.",selection.highest_generation,generation))
        } else {
            Ok(())
        }
    }
    fn check_trust(selection: &Selection) -> Result<u64, String> {
        let folder = selection.folder.as_ref().ok_or("Общая папка не выбрана.")?;
        let head = shared_folder::head(folder)?;
        Self::check_generation(selection, head)?;
        if selection.highest_generation > 0 {
            let digest = shared_folder::generation_digest(folder, selection.highest_generation)?;
            if selection.highest_digest.is_none() || selection.highest_digest != digest {
                return Err("Подтверждённое поколение общей базы изменилось. Проверьте выбранную общую папку.".into());
            }
        }
        Ok(head)
    }
    fn pull(&self, state: &mut State) -> Result<(), String> {
        let folder = state
            .selection
            .folder
            .as_ref()
            .ok_or("Общая папка не выбрана.")?;
        Self::check_trust(&state.selection)?;
        let checkout = Checkout::open(folder, &self.cache_dir(state)?)?;
        Self::check_trust(&state.selection)?;
        Self::check_generation(&state.selection, checkout.generation())?;
        state.selection.highest_generation = checkout.generation();
        state.selection.highest_digest = checkout.verified_digest();
        self.save_selection(&state.selection)?;
        if let Some(config) = &state.selection.config {
            checkout.storage.install_local_config(config)?;
        }
        *self.sessions.lock().map_err(err)? = Arc::clone(&checkout.storage.folder_import);
        state.checkout = Some(checkout);
        state.available = true;
        state.error = None;
        Ok(())
    }
    fn snapshot(state: &State) -> WorkspaceStatus {
        WorkspaceStatus {
            mode: if state.selection.folder.is_some() {
                "shared"
            } else {
                "personal"
            }
            .into(),
            folder: state
                .selection
                .folder
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
            generation: state.checkout.as_ref().map(Checkout::generation),
            available: state.available,
            error: state.error.clone(),
            pending: state.pending.is_some(),
            workspace_token: state.token.clone(),
        }
    }
    pub fn status(&self) -> Result<WorkspaceStatus, String> {
        let state = self.state.lock().map_err(err)?;
        Ok(Self::snapshot(&state))
    }
    /// Only the native OS folder picker grants paths; peers and JS paths cannot.
    pub fn grant(&self, folder: PathBuf) -> Result<(), String> {
        let folder = fs::canonicalize(folder).map_err(err)?;
        if !folder.is_dir() {
            return Err("Выберите существующую общую папку.".into());
        }
        self.state.lock().map_err(err)?.grants.push(folder);
        Ok(())
    }
    pub fn connect(&self, folder: &Path) -> Result<WorkspaceStatus, String> {
        let folder = fs::canonicalize(folder).map_err(err)?;
        let mut state = self.state.lock().map_err(err)?;
        if state.pending.is_some() {
            return Err("Сначала подтвердите подготовленную публикацию общей базы.".into());
        }
        let grant = state
            .grants
            .iter()
            .position(|path| path == &folder)
            .ok_or("Сначала выберите общую папку в системном диалоге.")?;
        let key = format!("{:x}", Sha256::digest(folder.to_string_lossy().as_bytes()));
        let mut selection: Selection = match fs::read(
            self.root
                .join("shared-cache")
                .join(key)
                .join("workspace.json"),
        ) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(err)?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Selection {
                folder: Some(folder.clone()),
                config: None,
                highest_generation: 0,
                highest_digest: None,
            },
            Err(e) => return Err(err(e)),
        };
        if selection.folder.as_ref() != Some(&folder) {
            return Err("Локальные настройки относятся к другой общей папке.".into());
        }
        // No personal records or local Excel paths are copied into a new team.
        let key = format!("{:x}", Sha256::digest(folder.to_string_lossy().as_bytes()));
        let cache = self.root.join("shared-cache").join(key);
        fs::create_dir_all(&cache).map_err(err)?;
        if cache.join("pending").exists() {
            return Err("В этой папке осталась неподтверждённая публикация. Перезапустите приложение с прежним подключением.".into());
        }
        Self::check_trust(&selection)?;
        let checkout = Checkout::open(&folder, &cache)?;
        Self::check_trust(&selection)?;
        Self::check_generation(&selection, checkout.generation())?;
        selection.highest_generation = checkout.generation();
        selection.highest_digest = checkout.verified_digest();
        if let Some(config) = &selection.config {
            checkout.storage.install_local_config(config)?;
        }
        self.save_selection(&selection)?;
        state.grants.remove(grant);
        state.selection = selection;
        *self.sessions.lock().map_err(err)? = Arc::clone(&checkout.storage.folder_import);
        state.checkout = Some(checkout);
        state.available = true;
        state.error = None;
        state.token = token();
        Ok(Self::snapshot(&state))
    }
    pub fn disconnect(&self, expected: Option<&str>) -> Result<WorkspaceStatus, String> {
        let mut state = self.state.lock().map_err(err)?;
        if Some(state.token.as_str()) != expected {
            return Err("Выбранная база изменилась. Повторите переключение.".into());
        }
        if state.pending.is_some() {
            return Err("Сначала подтвердите подготовленную публикацию общей базы.".into());
        }
        self.save_selection(&Selection::default())?;
        state.selection = Selection::default();
        *self.sessions.lock().map_err(err)? = Arc::clone(&state.personal.folder_import);
        state.checkout = None;
        state.available = true;
        state.error = None;
        state.token = token();
        Ok(Self::snapshot(&state))
    }
    fn unavailable(state: &mut State, error: String) {
        if state.available {
            state.token = token();
        }
        state.available = false;
        state.error = Some(error);
    }
    fn latest(&self, state: &mut State) -> Result<(), String> {
        if state.selection.folder.is_none() {
            return Ok(());
        }
        if state.pending.is_some() {
            return Err("Публикация не подтверждена. Повторите синхронизацию; новые изменения заблокированы.".into());
        }
        let head = match Self::check_trust(&state.selection) {
            Ok(head) => head,
            Err(error) => {
                Self::unavailable(state, error.clone());
                return Err(error);
            }
        };
        if let Err(error) = Self::check_generation(&state.selection, head) {
            Self::unavailable(state, error.clone());
            return Err(error);
        }
        if !state.available || state.checkout.as_ref().map(Checkout::generation) != Some(head) {
            if let Err(error) = self.pull(state) {
                Self::unavailable(state, error.clone());
                return Err(error);
            }
            state.token = token();
            return Err(
                "Общая база обновилась. Обновите данные и повторите выбор или предпросмотр.".into(),
            );
        }
        Ok(())
    }
    pub fn sync(&self) -> Result<WorkspaceStatus, String> {
        let mut state = self.state.lock().map_err(err)?;
        if state.selection.folder.is_none() {
            return Ok(Self::snapshot(&state));
        }
        if state.pending.is_some() {
            if let Err(error) = self.finish_pending(&mut state) {
                Self::unavailable(&mut state, error);
                return Ok(Self::snapshot(&state));
            }
        }
        if let Err(error) = self.latest(&mut state) {
            if !state.available {
                state.error = Some(error);
            }
        }
        Ok(Self::snapshot(&state))
    }
    pub fn discard_pending(
        &self,
        workspace_token: Option<&str>,
    ) -> Result<WorkspaceStatus, String> {
        let mut state = self.state.lock().map_err(err)?;
        if workspace_token != Some(state.token.as_str()) {
            return Err("Выбранная база изменилась. Повторите проверку состояния.".into());
        }
        if state.pending.is_none() {
            return Err("Нет неподтверждённой публикации.".into());
        }
        let pending = self.pending_dir(&state)?;
        if pending.exists() {
            fs::rename(
                &pending,
                self.cache_dir(&state)?
                    .join(format!("abandoned-{}", token())),
            )
            .map_err(err)?;
        }
        state.pending = None;
        state.token = token();
        if let Err(error) = self.pull(&mut state) {
            Self::unavailable(&mut state, error);
        }
        Ok(Self::snapshot(&state))
    }
    fn finish_pending(&self, state: &mut State) -> Result<(), String> {
        let directory = self.pending_dir(state)?;
        let pending = state
            .pending
            .as_mut()
            .ok_or("Нет подготовленной публикации.")?;
        if pending.publication.is_none() {
            let candidate = pending
                .candidate
                .as_ref()
                .ok_or("Рабочая копия публикации недоступна.")?;
            let publication = candidate.prepare_publication()?;
            fs::create_dir_all(&directory).map_err(err)?;
            publication.persist(&directory)?;
            pending.publication = Some(publication);
        }
        let generation = pending.publication.as_ref().unwrap().publish()?;
        if generation >= state.selection.highest_generation {
            state.selection.highest_generation = generation;
            state.selection.highest_digest = Some(pending.publication.as_ref().unwrap().digest());
        }
        if let Some(mut candidate) = pending.candidate.take() {
            candidate.confirm_publication(generation, pending.publication.as_ref().unwrap())?;
            state.checkout = Some(candidate);
        } else {
            self.pull(state)?;
        }
        self.save_selection(&state.selection)?;
        // Remove only this runtime's private publication journal after confirmation.
        fs::remove_dir_all(&directory).map_err(err)?;
        state.pending = None;
        state.available = true;
        state.error = None;
        Ok(())
    }
    pub fn execute<T>(
        &self,
        workspace_token: Option<&str>,
        access: Access,
        action: impl FnOnce(&Storage) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut state = self.state.lock().map_err(err)?;
        if workspace_token != Some(state.token.as_str()) {
            return Err("Выбранная база изменилась. Обновите данные и повторите действие.".into());
        }
        self.latest(&mut state)?;
        if !state.available {
            return Err(state
                .error
                .clone()
                .unwrap_or_else(|| "База недоступна.".into()));
        }
        if state.selection.folder.is_none() {
            return action(&state.personal);
        }
        if access == Access::PersonalOnly {
            return Err("Восстановление при перезапуске доступно только для личной базы.".into());
        }
        if access != Access::Write {
            let result = action(
                &state
                    .checkout
                    .as_ref()
                    .ok_or("Общая база недоступна.")?
                    .storage,
            );
            if access == Access::Local {
                let config = state.checkout.as_ref().unwrap().storage.local_config()?;
                state.selection.config = Some(config);
                self.save_selection(&state.selection)?;
            }
            return result;
        }
        let checkout = state.checkout.as_ref().ok_or("Общая база недоступна.")?;
        let mut candidate = checkout.fork(&self.cache_dir(&state)?)?;
        candidate.storage.folder_import = Arc::clone(&checkout.storage.folder_import);
        candidate.storage.folder_import_serial.store(
            checkout.storage.folder_import_serial.load(Ordering::SeqCst),
            Ordering::SeqCst,
        );
        let value = action(&candidate.storage)?;
        state.selection.config = Some(candidate.storage.local_config()?);
        self.save_selection(&state.selection)?;
        state.pending = Some(Pending {
            candidate: Some(candidate),
            publication: None,
        });
        if let Err(error) = self.finish_pending(&mut state) {
            Self::unavailable(&mut state, error.clone());
            return Err(format!("Изменения пока не подтверждены общей папкой: {error} Повторите синхронизацию; не повторяйте импорт."));
        }
        Ok(value)
    }
    /// Keep the schedule and its verified snapshots local to this installation.
    pub fn scheduled_backup(
        &self, workspace_token: Option<&str>, enabled: Option<bool>,
    ) -> Result<super::ScheduledBackupStatus, String> {
        let key = {
            let state = self.state.lock().map_err(err)?;
            if workspace_token != Some(state.token.as_str()) {
                return Err("Выбранная база изменилась. Обновите данные и повторите действие.".into());
            }
            match state.selection.folder.as_ref() {
                Some(folder) => format!("shared-{:x}", Sha256::digest(folder.to_string_lossy().as_bytes())),
                None => "personal".into(),
            }
        };
        self.execute(workspace_token, Access::Read, |store| {
            store.scheduled_backup(&self.root, &key, enabled)
        })
    }
    /// Preview existing local reports against the currently connected team.
    pub fn preview_personal_transfer(
        &self, workspace_token: Option<&str>, page: usize,
    ) -> Result<TransferPreview, String> {
        let personal_root = {
            let state = self.state.lock().map_err(err)?;
            if state.selection.folder.is_none() {
                return Err("Подключите общую базу для переноса личных отчётов.".into());
            }
            state.personal.root.clone()
        };
        self.execute(workspace_token, Access::Read, move |shared| {
            let personal = Storage::new(personal_root)?;
            shared.personal_transfer_preview(&personal, page)
        })
    }

    pub fn apply_personal_transfer(
        &self, workspace_token: Option<&str>,
        source_revision: i64, target_revision: i64, selected_ids: Vec<String>,
    ) -> Result<TransferReceipt, String> {
        let personal_root = {
            let state = self.state.lock().map_err(err)?;
            if state.selection.folder.is_none() {
                return Err("Перенос доступен только при подключённой общей базе.".into());
            }
            state.personal.root.clone()
        };
        let backup_dir = self.root.join("transfer-backups");
        self.execute(workspace_token, Access::Write, move |shared| {
            let personal = Storage::new(personal_root)?;
            shared.import_selected_personal_reports(
                &personal,source_revision,target_revision,&selected_ids,&backup_dir
            )
        })
    }

    pub fn close_window(&self, owner: &str) {
        // Cancellation is independent of the data lock: an import may hold that
        // lock while waiting for SQLite or parsing a large workbook.
        if let Ok(sessions) = self.sessions.lock() {
            if let Ok(mut session) = sessions.lock() {
                if session
                    .as_ref()
                    .is_some_and(|current| current.owner == owner)
                {
                    if let Some(previous) = session.take() {
                        if let Some(active) = previous.active {
                            active.cancel();
                        }
                    }
                }
            }
        }
    }
}
impl Storage {
    fn local_config(&self) -> Result<Config, String> {
        self.config(&self.connect()?)
    }
    fn install_local_config(&self, config: &Config) -> Result<(), String> {
        let mut connection = self.connect()?;
        let tx = connection.transaction().map_err(err)?;
        super::insert_key(&tx, "config", &serde_json::to_string(config).map_err(err)?)?;
        tx.commit().map_err(err)
    }
}
#[cfg(test)]
#[path = "shared_runtime_tests.rs"]
mod tests;
