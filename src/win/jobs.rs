use super::{
    icons::{self, Icon, Spec},
    shell, ui,
    update::{self, Release},
};
use appstack::{model::*, storage::Store};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, mpsc},
    thread,
};
use windows::Win32::{Foundation::*, UI::WindowsAndMessaging::PostMessageW};

pub enum Job {
    Save(Config),
    Import(PathBuf),
    Export(Config, PathBuf),
    Launch(Vec<Entry>),
    Shortcut(Group, bool),
    Synchronize(Config),
    EnsureBrand(Config),
    Startup(bool),
    /// Whether the user asked, so an automatic check stays quiet unless it finds something.
    CheckUpdate(bool),
    Update(Release),
    /// Refreshes the community styles cache; failures keep the previous copy silently.
    FetchStyles,
    Stop,
}

pub enum Completed {
    Saved,
    Imported(Result<Vec<Group>, String>),
    Exported(Result<(), String>),
    Shortcut(Result<PathBuf, String>),
    Icon {
        generation: u64,
        key: u64,
        result: Result<(Icon, PathBuf), String>,
    },
    Error(String),
    Status(String),
    Release(bool, Result<Option<Release>, String>),
    /// The command that installs the downloaded update once AppStack has exited.
    Updated(Result<std::process::Command, String>),
}

pub struct Background {
    pub jobs: mpsc::Sender<Job>,
    pub icons: mpsc::Sender<(u64, Spec)>,
    pub completed: mpsc::Receiver<Completed>,
    worker: Option<thread::JoinHandle<()>>,
}

pub fn group_icon_path(store: &Store, group: &Group) -> PathBuf {
    store
        .root
        .join("group-icons")
        .join(format!("{}-{}.ico", group.id, crate::LOGO_REVISION))
}

fn deliver(sender: &mpsc::Sender<Completed>, owner: usize, result: Completed) {
    if sender.send(result).is_ok() {
        unsafe {
            let _ = PostMessageW(Some(HWND(owner as *mut _)), ui::ASYNC, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn group_spec(config: &Config, group: &Group, size: u32, grayscale: bool) -> Spec {
    let paths = if !group.appearance.grid_icon && !group.appearance.icon.is_empty() {
        vec![group.appearance.icon.clone()]
    } else {
        group
            .ordered_entries()
            .into_iter()
            .take(4)
            .map(|entry| {
                if !entry.icon.is_empty() {
                    entry.icon.clone()
                } else {
                    match &entry.target {
                        Target::Application(path) => path.clone(),
                        Target::Group(id) => config
                            .group(id)
                            .map(|group| group.appearance.icon.clone())
                            .unwrap_or_default(),
                    }
                }
            })
            .collect()
    };
    Spec {
        paths,
        composite: group.appearance.grid_icon,
        mosaic: group.appearance.grid_icon && group.appearance.mosaic_icon,
        grayscale,
        size,
    }
}

pub fn entry_spec(config: &Config, entry: &Entry, size: u32) -> Spec {
    if !entry.icon.is_empty() {
        Spec {
            paths: vec![entry.icon.clone()],
            composite: false,
            mosaic: false,
            grayscale: false,
            size,
        }
    } else {
        match &entry.target {
            Target::Application(path) => Spec {
                paths: vec![path.clone()],
                composite: false,
                mosaic: false,
                grayscale: false,
                size,
            },
            Target::Group(id) => config
                .group(id)
                .map(|group| group_spec(config, group, size, false))
                .unwrap_or(Spec {
                    paths: Vec::new(),
                    composite: false,
                    mosaic: false,
                    grayscale: false,
                    size,
                }),
        }
    }
}

impl Background {
    pub fn new(owner: HWND, store: Store) -> Self {
        let owner = owner.0 as usize;
        let (jobs, receiver) = mpsc::channel();
        let (icons, icon_receiver) = mpsc::channel::<(u64, Spec)>();
        let (sender, completed) = mpsc::channel();
        let icon_receiver = Arc::new(Mutex::new(icon_receiver));
        for _ in 0..2 {
            let receiver = Arc::clone(&icon_receiver);
            let sender = sender.clone();
            let cache = store.root.join("icon-cache");
            thread::spawn(move || {
                let _apartment = shell::Apartment::new();
                loop {
                    let request = match receiver.lock() {
                        Ok(receiver) => receiver.recv(),
                        Err(_) => break,
                    };
                    let Ok((generation, spec)) = request else {
                        break;
                    };
                    let result = icons::load(&spec, &cache);
                    deliver(
                        &sender,
                        owner,
                        Completed::Icon {
                            generation,
                            key: spec.key(),
                            result,
                        },
                    );
                }
            });
        }
        let worker = thread::spawn(move || {
            let _apartment = shell::Apartment::new();
            while let Ok(job) = receiver.recv() {
                if matches!(&job, Job::EnsureBrand(_))
                    && std::fs::read_to_string(store.root.join("branding.version"))
                        .is_ok_and(|revision| revision == crate::LOGO_REVISION)
                {
                    icons::prune_cache(&store.root.join("icon-cache"));
                    continue;
                }
                let completed = match job {
                    Job::Save(config) => match store.save(&config) {
                        Ok(()) => Completed::Saved,
                        Err(error) => {
                            Completed::Error(format!("Could not save your changes: {error}"))
                        }
                    },
                    Job::Import(path) => Completed::Imported(store.import(&path)),
                    Job::Export(config, path) => Completed::Exported(store.export(&config, &path)),
                    Job::Launch(entries) => {
                        let errors: Vec<_> = entries
                            .iter()
                            .filter_map(|entry| shell::launch(entry).err())
                            .collect();
                        if errors.is_empty() {
                            Completed::Status(String::new())
                        } else {
                            Completed::Error(errors.join("\n"))
                        }
                    }
                    Job::Shortcut(group, desktop) => {
                        let icon = group_icon_path(&store, &group);
                        let result = shell::shortcut(
                            &store,
                            &group,
                            icon.is_file().then_some(icon.as_path()),
                        )
                        .and_then(|path| {
                            if desktop {
                                let desktop = shell::desktop()?;
                                let filename = path.file_name().ok_or("Shortcut has no name")?;
                                let output = desktop.join(filename);
                                if output.exists() {
                                    return Err(
                                        "A shortcut with this name already exists on the desktop"
                                            .into(),
                                    );
                                }
                                std::fs::copy(&path, &output).map_err(shell::failure)?;
                            }
                            Ok(path)
                        });
                        Completed::Shortcut(result)
                    }
                    Job::Synchronize(config) | Job::EnsureBrand(config) => {
                        let directory = store.root.join("group-icons");
                        let _ = std::fs::create_dir_all(&directory);
                        let mut errors = Vec::new();
                        for group in &config.groups {
                            let spec =
                                group_spec(&config, group, 64, config.settings.grayscale_shortcuts);
                            let path = group_icon_path(&store, group);
                            let icon = icons::load(&spec, &store.root.join("icon-cache"));
                            if let Ok((_, cached)) = icon {
                                let _ = std::fs::copy(cached, &path);
                            }
                            if let Err(error) = shell::shortcut(
                                &store,
                                group,
                                path.is_file().then_some(path.as_path()),
                            ) {
                                errors.push(error);
                            }
                            if let Err(error) = shell::update_jump_list(
                                &store,
                                group,
                                config.settings.launch_all_enabled,
                            ) {
                                errors.push(error);
                            }
                        }
                        icons::prune_cache(&store.root.join("icon-cache"));
                        if errors.is_empty() {
                            let _ = std::fs::write(
                                store.root.join("branding.version"),
                                crate::LOGO_REVISION,
                            );
                        }
                        Completed::Status(
                            errors
                                .first()
                                .map(|error| format!("Shell integration: {error}"))
                                .unwrap_or_default(),
                        )
                    }
                    Job::Startup(enabled) => match shell::startup(&store, enabled) {
                        Ok(()) => Completed::Status(String::new()),
                        Err(error) => Completed::Error(error),
                    },
                    // Network waits get their own thread so saves are never queued behind them.
                    Job::CheckUpdate(manual) => {
                        let sender = sender.clone();
                        thread::spawn(move || {
                            deliver(&sender, owner, Completed::Release(manual, update::latest()))
                        });
                        continue;
                    }
                    Job::FetchStyles => {
                        let root = store.root.clone();
                        thread::spawn(move || {
                            let _ = update::community_styles(&root);
                        });
                        continue;
                    }
                    Job::Update(release) => {
                        let (sender, root) = (sender.clone(), store.root.clone());
                        thread::spawn(move || {
                            deliver(
                                &sender,
                                owner,
                                Completed::Updated(update::prepare(&release, &root)),
                            )
                        });
                        continue;
                    }
                    Job::Stop => break,
                };
                deliver(&sender, owner, completed);
            }
        });
        Self {
            jobs,
            icons,
            completed,
            worker: Some(worker),
        }
    }
}

impl Drop for Background {
    fn drop(&mut self) {
        let _ = self.jobs.send(Job::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
