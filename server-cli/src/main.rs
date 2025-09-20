#![deny(unsafe_code)]
#![deny(clippy::clone_on_ref_ptr)]

#[cfg(all(
    target_os = "windows",
    not(feature = "hot-agent"),
    not(feature = "hot-site"),
))]
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

/// `server-cli` interface commands not to be confused with the commands sent
/// from the client to the server
mod cli;
mod settings;
mod shutdown_coordinator;
mod tui_runner;
mod tuilog;
mod web;
use crate::{
    cli::{
        Admin, ArgvApp, ArgvCommand, BenchParams, Message, MessageReturn, SharedCommand, Shutdown,
    },
    settings::Settings,
    shutdown_coordinator::ShutdownCoordinator,
    tui_runner::Tui,
    tuilog::TuiLog,
};
use common::{
    clock::Clock,
    comp::{ChatType, Player},
    consts::MIN_RECOMMENDED_TOKIO_THREADS,
};
use common_base::span;
use core::sync::atomic::{AtomicUsize, Ordering};
use rand::distr::SampleString;
use server::{Event, Input, Server, persistence::DatabaseSettings, settings::Protocol};
use std::{
    io,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
use tokio::sync::Notify;
use tracing::{info, trace};

lazy_static::lazy_static! {
    pub static ref LOG: TuiLog<'static> = TuiLog::default();
}
const TPS: u64 = 30;

fn main() -> io::Result<()> {
    #[cfg(feature = "tracy")]
    common_base::tracy_client::Client::start();

    use clap::Parser;
    let app = ArgvApp::parse();

    let basic = !app.tui || app.command.is_some();
    let noninteractive = app.non_interactive;
    let no_auth = app.no_auth;
    let sql_log_mode = app.sql_log_mode;

    // noninteractive implies basic
    let basic = basic || noninteractive;

    let shutdown_signal = Arc::new(AtomicBool::new(false));

    let (_guards, _guards2) = if basic {
        (Vec::new(), common_frontend::init_stdout(None))
    } else {
        (common_frontend::init(None, &|| LOG.clone()), Vec::new())
    };

    // Load settings
    let settings = settings::Settings::load();

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        for signal in &settings.shutdown_signals {
            let _ = signal_hook::flag::register(signal.to_signal(), Arc::clone(&shutdown_signal));
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    if !settings.shutdown_signals.is_empty() {
        tracing::warn!(
            "Server configuration contains shutdown signals, but your platform does not support \
             them"
        );
    }

    // Determine folder to save server data in
    let server_data_dir = {
        let mut path = common_base::userdata_dir_workspace!();
        info!("Using userdata folder at {}", path.display());
        path.push(server::DEFAULT_DATA_DIR_NAME);
        path
    };

    // We don't need that many threads in the async pool, at least 2 but generally
    // 25% of all available will do
    // TODO: evaluate std::thread::available_concurrency as a num_cpus replacement
    let runtime = Arc::new(
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads((num_cpus::get() / 4).max(MIN_RECOMMENDED_TOKIO_THREADS))
            .thread_name_fn(|| {
                static ATOMIC_ID: AtomicUsize = AtomicUsize::new(0);
                let id = ATOMIC_ID.fetch_add(1, Ordering::SeqCst);
                format!("tokio-server-{}", id)
            })
            .build()
            .unwrap(),
    );

    #[cfg(feature = "hot-agent")]
    {
        agent::init();
    }
    #[cfg(feature = "hot-site")]
    {
        world::init();
    }

    // Load server settings
    let mut server_settings = server::Settings::load(&server_data_dir);
    let mut editable_settings = server::EditableSettings::load(&server_data_dir);

    // Apply no_auth modifier to the settings
    if no_auth {
        server_settings.auth_server_address = None;
    }

    // Relative to data_dir
    const PERSISTENCE_DB_DIR: &str = "saves";

    let database_settings = DatabaseSettings {
        db_dir: server_data_dir.join(PERSISTENCE_DB_DIR),
        sql_log_mode,
    };

    let mut bench = None;
    if let Some(command) = app.command {
        match command {
            ArgvCommand::Shared(SharedCommand::Admin { command }) => {
                let login_provider = server::login_provider::LoginProvider::new(
                    server_settings.auth_server_address,
                    runtime,
                );

                return match command {
                    Admin::Add { username, role } => {
                        // FIXME: Currently the UUID can get returned even if the file didn't
                        // change, so this can't be relied on as an error
                        // code; moreover, we do nothing with the UUID
                        // returned in the success case.  Fix the underlying function to return
                        // enough information that we can reliably return an error code.
                        let _ = server::add_admin(
                            &username,
                            role,
                            &login_provider,
                            &mut editable_settings,
                            &server_data_dir,
                        );
                        Ok(())
                    },
                    Admin::Remove { username } => {
                        // FIXME: Currently the UUID can get returned even if the file didn't
                        // change, so this can't be relied on as an error
                        // code; moreover, we do nothing with the UUID
                        // returned in the success case.  Fix the underlying function to return
                        // enough information that we can reliably return an error code.
                        let _ = server::remove_admin(
                            &username,
                            &login_provider,
                            &mut editable_settings,
                            &server_data_dir,
                        );
                        Ok(())
                    },
                };
            },
            ArgvCommand::Bench(params) => {
                bench = Some(params);
                // If we are trying to benchmark, don't limit the server view distance.
                server_settings.max_view_distance = None;
                // TODO: add setting to adjust wildlife spawn density, note I
                // tried but Index setup makes it a bit
                // annoying, might require a more involved refactor to get
                // working nicely
            },
        };
    }

    // Panic hook to ensure that console mode is set back correctly if in non-basic
    // mode
    if !basic {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            Tui::shutdown(basic);
            hook(info);
        }));
    }

    let tui = (!noninteractive).then(|| Tui::run(basic));

    info!("Starting server...");

    let protocols_and_addresses = server_settings.gameserver_protocols.clone();
    let web_port = &settings.web_address.port();
    // Create server
    #[cfg_attr(not(feature = "worldgen"), expect(unused_mut))]
    let mut server = Server::new(
        server_settings,
        editable_settings,
        database_settings,
        &server_data_dir,
        &|_| {},
        Arc::clone(&runtime),
    )
    .expect("Failed to create server instance!");

    let registry = Arc::clone(server.metrics_registry());
    let chat = server.chat_cache().clone();
    let metrics_shutdown = Arc::new(Notify::new());
    let metrics_shutdown_clone = Arc::clone(&metrics_shutdown);
    let web_chat_secret = settings.web_chat_secret.clone();
    let ui_api_secret = settings.ui_api_secret.clone().unwrap_or_else(|| {
        // when no secret is provided we generate one that we distribute via the /ui
        // endpoint
        use rand::distr::Alphanumeric;
        Alphanumeric.sample_string(&mut rand::rng(), 32)
    });

    let (web_ui_request_s, web_ui_request_r) = tokio::sync::mpsc::channel(1000);

    runtime.spawn(async move {
        web::run(
            registry,
            chat,
            web_chat_secret,
            ui_api_secret,
            web_ui_request_s,
            settings.web_address,
            metrics_shutdown_clone.notified(),
        )
        .await
    });

    // Collect addresses that the server is listening to log.
    let gameserver_addresses = protocols_and_addresses
        .into_iter()
        .map(|protocol| match protocol {
            Protocol::Tcp { address } => ("TCP", address),
            Protocol::Quic {
                address,
                cert_file_path: _,
                key_file_path: _,
            } => ("QUIC", address),
        });

    info!(
        ?web_port,
        ?gameserver_addresses,
        "Server is ready to accept connections."
    );

    #[cfg(feature = "worldgen")]
    if let Some(bench) = bench {
        server.create_centered_persister(bench.view_distance);
    }

    server_loop(
        server,
        bench,
        settings,
        tui,
        web_ui_request_r,
        shutdown_signal,
    )?;

    metrics_shutdown.notify_one();

    Ok(())
}

fn server_loop(
    mut server: Server,
    bench: Option<BenchParams>,
    settings: Settings,
    tui: Option<Tui>,
    mut web_ui_request_r: tokio::sync::mpsc::Receiver<(
        Message,
        tokio::sync::oneshot::Sender<MessageReturn>,
    )>,
    shutdown_signal: Arc<AtomicBool>,
) -> io::Result<()> {
    // Set up an fps clock
    let mut clock = Clock::new(Duration::from_secs_f64(1.0 / TPS as f64));
    let mut shutdown_coordinator = ShutdownCoordinator::new(Arc::clone(&shutdown_signal));
    let mut bench_exit_time = None;

    let mut tick_no = 0u64;
    'outer: loop {
        span!(guard, "work");
        if let Some(bench) = bench {
            if let Some(t) = bench_exit_time {
                if Instant::now() > t {
                    break;
                }
            } else if tick_no != 0 && !server.chunks_pending() {
                println!("Chunk loading complete");
                bench_exit_time = Some(Instant::now() + Duration::from_secs(bench.duration.into()));
            }
        };

        tick_no += 1;
        // Terminate the server if instructed to do so by the shutdown coordinator
        if shutdown_coordinator.check(&mut server, &settings) {
            break;
        }

        let events = server
            .tick(Input::default(), clock.dt())
            .expect("Failed to tick server");

        for event in events {
            match event {
                Event::ClientConnected { entity: _ } => info!("Client connected!"),
                Event::ClientDisconnected { entity: _ } => info!("Client disconnected!"),
                Event::Chat { entity: _, msg } => info!("[Client] {}", msg),
            }
        }

        // Clean up the server after a tick.
        server.cleanup();

        if tick_no.rem_euclid(1000) == 0 {
            trace!(?tick_no, "keepalive")
        }

        let mut handle_msg = |msg, response: tokio::sync::oneshot::Sender<MessageReturn>| {
            use specs::{Join, WorldExt};
            match msg {
                Message::Shutdown {
                    command: Shutdown::Cancel,
                } => shutdown_coordinator.abort_shutdown(&mut server),
                Message::Shutdown {
                    command: Shutdown::Graceful { seconds, reason },
                } => {
                    shutdown_coordinator.initiate_shutdown(
                        &mut server,
                        Duration::from_secs(seconds),
                        reason,
                    );
                },
                Message::Shutdown {
                    command: Shutdown::Immediate,
                } => {
                    return true;
                },
                Message::Shared(SharedCommand::Admin {
                    command: Admin::Add { username, role },
                }) => {
                    server.add_admin(&username, role);
                },
                Message::Shared(SharedCommand::Admin {
                    command: Admin::Remove { username },
                }) => {
                    server.remove_admin(&username);
                },
                #[cfg(feature = "worldgen")]
                Message::LoadArea { view_distance } => {
                    server.create_centered_persister(view_distance);
                },
                Message::SqlLogMode { mode } => {
                    server.set_sql_log_mode(mode);
                },
                Message::DisconnectAllClients => {
                    server.disconnect_all_clients();
                },
                Message::ListPlayers => {
                    let players: Vec<String> = server
                        .state()
                        .ecs()
                        .read_storage::<Player>()
                        .join()
                        .map(|p| p.alias.clone())
                        .collect();
                    let _ = response.send(MessageReturn::Players(players));
                },
                Message::ListLogs => {
                    let log = LOG.inner.lock().unwrap();
                    let lines: Vec<_> = log
                        .lines
                        .iter()
                        .rev()
                        .take(30)
                        .map(|l| l.to_string())
                        .collect();
                    let _ = response.send(MessageReturn::Logs(lines));
                },
                Message::SendGlobalMsg { msg } => {
                    use server::state_ext::StateExt;
                    let msg = ChatType::Meta.into_plain_msg(msg);
                    server.state().send_chat(msg, false);
                },
            }
            false
        };

        if let Some(tui) = tui.as_ref() {
            while let Ok(msg) = tui.msg_r.try_recv() {
                let (sender, mut recv) = tokio::sync::oneshot::channel();
                if handle_msg(msg, sender) {
                    info!("Closing the server");
                    break 'outer;
                }
                if let Ok(msg_answ) = recv.try_recv() {
                    match msg_answ {
                        MessageReturn::Players(players) => info!("Players: {:?}", players),
                        MessageReturn::Logs(_) => info!("skipp sending logs to tui"),
                    };
                }
            }
        }

        while let Ok((msg, sender)) = web_ui_request_r.try_recv() {
            if handle_msg(msg, sender) {
                info!("Closing the server");
                break 'outer;
            }
        }

        drop(guard);
        // Wait for the next tick.
        clock.tick();
        #[cfg(feature = "tracy")]
        common_base::tracy_client::frame_mark();
    }
    Ok(())
}
