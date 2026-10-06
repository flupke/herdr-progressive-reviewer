use std::path::PathBuf;

use review_repository::repository::{ChangeId, RepoType};
use review_source::ReviewCheckpoint;
use review_test_support::repository_fixture;
use review_threads::ThreadCommand;
use review_ui::SourceLoadMode;
use ui_events::{
    DiffContentLoaded, ExploreRestored, RepositoryFilesChanged, RepositoryMetadataChanged,
    RepositoryRefreshFinished, ReviewThreadsLoaded, RevisionEditFailed, SourceContentLoaded,
};

use super::fixture::EffectsFixture;
use super::*;

fn load_diff(checkpoint: &ReviewCheckpoint, path: &str) -> Action {
    Action::Document(DocumentAction::Load(DocumentLoad::Diff {
        review_checkpoint: checkpoint.clone(),
        path: path.to_owned(),
    }))
}

fn event_names(events: &[EventEnvelope]) -> Vec<&'static str> {
    events
        .iter()
        .filter_map(|event| {
            if event.downcast_ref::<RepositoryMetadataChanged>().is_some() {
                Some("metadata")
            } else if event.downcast_ref::<RepositoryFilesChanged>().is_some() {
                Some("files")
            } else if event.downcast_ref::<ExploreRestored>().is_some() {
                Some("explore restored")
            } else if event.downcast_ref::<RepositoryRefreshFinished>().is_some() {
                Some("refresh finished")
            } else {
                None
            }
        })
        .collect()
}

#[test_case::test_case(RepoType::Git; "git")]
#[test_case::test_case(RepoType::Jj; "jj")]
fn a_refresh_publishes_files_the_documents_can_load_then_restores_explore(kind: RepoType) {
    let files = repository_fixture(kind);
    files.write("src/lib.rs", b"pub fn refreshed() {}\n");
    let mut fixture = EffectsFixture::start(files, |_| {});

    let events = fixture.refresh();

    assert_eq!(
        event_names(&events),
        ["metadata", "files", "explore restored", "refresh finished"]
    );
    let files = events
        .iter()
        .find_map(|event| event.downcast_ref::<RepositoryFilesChanged>())
        .unwrap();
    // The files event can only be acted on once the documents hold its snapshot.
    fixture.perform([load_diff(&files.review_checkpoint, "src/lib.rs")]);
    let loaded = fixture.wait_for::<DiffContentLoaded>();
    assert_eq!(loaded.review_checkpoint, files.review_checkpoint);
    assert_eq!(
        loaded.new_content.as_deref(),
        Some(b"pub fn refreshed() {}\n".as_slice())
    );

    fixture.files.write("src/lib.rs", b"pub fn changed() {}\n");
    let events = fixture.refresh();
    // The same review keeps its restored Explore round.
    assert_eq!(
        event_names(&events),
        ["metadata", "files", "refresh finished"]
    );
}

#[test_case::test_case("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz", "immutable or unavailable"; "immutable root")]
#[test_case::test_case("unknownchange", "read jj repository failed"; "unknown change")]
fn a_failed_revision_edit_reports_why_and_keeps_the_comparison(change_id: &str, reason: &str) {
    let files = repository_fixture(RepoType::Jj);
    files.write("src/lib.rs", b"pub fn current() {}\n");
    let mut fixture = EffectsFixture::start(files, |_| {});
    let checkpoint = fixture.refreshed_checkpoint();

    fixture.perform([Action::Repository(RepositoryAction::EditRevision {
        change_id: ChangeId::from(change_id.to_owned()),
    })]);

    let events = fixture.events_until::<RevisionEditFailed>();
    let failure = events.last().unwrap().downcast_ref::<RevisionEditFailed>();
    let message = failure.unwrap().message.clone().unwrap();
    assert!(message.contains(reason), "{message}");
    assert!(event_names(&events).is_empty(), "no refresh on failure");
    assert_eq!(fixture.refreshed_checkpoint(), checkpoint);
}

#[test]
fn documents_and_threads_do_not_wait_for_repository_work() {
    let files = repository_fixture(RepoType::Git);
    files.write("changed.rs", b"fn original() {}\n");
    files.new_change("original");
    files.write("changed.rs", b"fn updated() {}\n");
    let mut fixture = EffectsFixture::start(files, |_| {});
    let checkpoint = fixture.refreshed_checkpoint();
    let hold = fixture.effects.hold_repository_work();
    fixture.effects.refresh().unwrap();

    fixture.perform([
        Action::Document(DocumentAction::Load(DocumentLoad::Diffs {
            review_checkpoint: checkpoint.clone(),
            paths: vec!["changed.rs".to_owned()],
        })),
        Action::Document(DocumentAction::Load(DocumentLoad::Source {
            snapshot_id: checkpoint.checkpoint.clone(),
            location: review_lsp::SourceLocation {
                path: PathBuf::from("changed.rs"),
                line: 0,
                byte_column: 0,
                end_line: 0,
                end_byte_column: 0,
            },
            mode: SourceLoadMode::External,
        })),
        Action::Thread(ThreadCommand::Load(checkpoint.review_unit.clone())),
    ]);

    let (mut diff, mut source, mut threads) = (None, None, false);
    while diff.is_none() || source.is_none() || !threads {
        let event = fixture.next_event();
        assert!(
            event_names(std::slice::from_ref(&event)).is_empty(),
            "the held refresh ran first"
        );
        diff = diff.or_else(|| event.downcast_ref::<DiffContentLoaded>().cloned());
        source = source.or_else(|| event.downcast_ref::<SourceContentLoaded>().cloned());
        threads |= event.downcast_ref::<ReviewThreadsLoaded>().is_some();
    }
    assert_eq!(
        diff.unwrap().new_content.as_deref(),
        Some(b"fn updated() {}\n".as_slice())
    );
    let source = source.unwrap();
    assert_eq!(source.content, b"fn updated() {}\n");
    assert_eq!(
        source.location.path,
        fixture.repository.root().join("changed.rs"),
        "relative paths resolve against the repository root"
    );
    drop(hold);
    fixture.wait_for::<RepositoryRefreshFinished>();
}

#[test]
fn settings_save_and_terminal_actions_run_in_order_until_quit() {
    let fixture = EffectsFixture::new(RepoType::Git);
    let mut opened = Vec::new();

    let flow = fixture
        .effects
        .perform_all(
            vec![
                Action::Settings(SettingsAction::SaveFilePaneWidth(42)),
                Action::Terminal(TerminalAction::OpenInEditor {
                    path: PathBuf::from("src/lib.rs"),
                    line: Some(7),
                }),
                Action::Terminal(TerminalAction::Quit),
                Action::Settings(SettingsAction::SaveFilePaneWidth(7)),
            ],
            &mut |action| match action {
                TerminalAction::OpenInEditor { path, line } => {
                    opened.push((path, line));
                    Ok(ControlFlow::Continue(()))
                }
                TerminalAction::CopyToClipboard(_) => Ok(ControlFlow::Continue(())),
                TerminalAction::Quit => Ok(ControlFlow::Break(())),
            },
        )
        .unwrap();

    assert_eq!(flow, ControlFlow::Break(()));
    assert_eq!(fixture.store.file_pane_width().unwrap(), Some(42));
    assert_eq!(opened, [(PathBuf::from("src/lib.rs"), Some(7))]);
}

#[test]
fn a_saved_writing_style_is_kept_for_the_next_round_and_shown_as_saved() {
    use review_explore_round_settings::WritingStyle;
    let mut fixture = EffectsFixture::new(RepoType::Git);

    fixture.perform([Action::Settings(SettingsAction::SaveExploreWritingStyle(
        WritingStyle::Plain,
    ))]);

    let ui_events::ExploreRoundSettingsLoaded(saved) =
        fixture.wait_for::<ui_events::ExploreRoundSettingsLoaded>();
    assert_eq!(saved.writing, WritingStyle::Plain);
    assert_eq!(saved, fixture.store.explore_round_settings().unwrap());
}

#[test]
fn a_saved_run_ahead_is_kept_and_shown_as_saved() {
    use review_explore_round_settings::RunAhead;
    let mut fixture = EffectsFixture::new(RepoType::Git);

    fixture.perform([Action::Settings(SettingsAction::SaveExploreRunAhead(
        RunAhead::Recommended,
    ))]);

    let ui_events::ExploreRoundSettingsLoaded(saved) =
        fixture.wait_for::<ui_events::ExploreRoundSettingsLoaded>();
    assert_eq!(saved.run_ahead, RunAhead::Recommended);
    assert_eq!(saved, fixture.store.explore_round_settings().unwrap());
}

#[test]
fn source_loads_prefer_frozen_content_when_a_deleted_path_is_recreated() {
    let files = repository_fixture(RepoType::Git);
    let deleted_content = b"fn deleted_from_worktree() {}\n";
    files.write("deleted.rs", deleted_content);
    files.new_change("add the file that the next change deletes");
    files.remove("deleted.rs");
    let mut fixture = EffectsFixture::start(files, |_| {});
    let checkpoint = fixture.refreshed_checkpoint();
    let path = fixture.repository.root().join("deleted.rs");
    std::fs::write(&path, "fn recreated_after_snapshot() {}\n").unwrap();
    let load = |mode| {
        Action::Document(DocumentAction::Load(DocumentLoad::Source {
            snapshot_id: checkpoint.checkpoint.clone(),
            location: review_lsp::SourceLocation {
                path: path.clone(),
                line: 0,
                byte_column: 0,
                end_line: 0,
                end_byte_column: 0,
            },
            mode,
        }))
    };

    fixture.perform([load(SourceLoadMode::External)]);
    let loaded = fixture.wait_for::<SourceContentLoaded>();
    assert_eq!(loaded.snapshot_id, checkpoint.checkpoint);
    assert_eq!(loaded.location.path, path);
    assert_eq!(loaded.mode, SourceLoadMode::External);
    assert_eq!(loaded.content, deleted_content);

    fixture.perform([load(SourceLoadMode::ThreadPeek)]);
    assert_eq!(
        fixture.wait_for::<SourceContentLoaded>().content,
        b"fn recreated_after_snapshot() {}\n"
    );

    std::fs::remove_file(&path).unwrap();
    fixture.perform([load(SourceLoadMode::ThreadPeek)]);
    fixture.wait_for::<ui_events::SourceContentLoadFailed>();
}

mod explore_page {
    use std::io::Read as _;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;
    use std::sync::mpsc;

    use herdr_client::protocol::WorkspaceId;
    use review_explore_page::{CommandRefusal, CommandSender, PageRound, RoundPublisher};
    use review_explore_page_host::{
        Browser, NetworkAccess, PageDirectory, PageHost, PageOpener, TunnelProgram, TunnelState,
    };
    use review_explore_page_settings::{ExplorePageSetting, PaneStarts};
    use review_ui::ExplorePageAction;
    use ui_events::{
        ExplorePageNotOpened, ExplorePageNotShared, ExplorePageOffNetwork,
        ExplorePageSettingsLoaded, ExplorePageShared, ExplorePageTunnel,
    };

    use review_test_support::GUARD;

    use crate::runtime::effects::fixture::test_fork_tools;
    use crate::runtime::page_sharing::PageSharing;

    use super::*;

    /// A reviewer's page, served for workspace `w1` under `state`.
    fn host(state: &Path) -> PageHost {
        let round = RoundPublisher::default();
        let commands = CommandSender::new(|_, reply| {
            reply.send(Err(CommandRefusal::Failed(
                "No session in this test".into(),
            )));
        });
        PageHost::start(
            PageRound::new(round.subscribe(), commands),
            &PageDirectory::new(state),
            &WorkspaceId("w1".into()),
            std::path::Path::new("/repositories/drafts"),
        )
        .unwrap()
    }

    /// A named pipe at `path`, and what its writers write until the last one closes it, read
    /// on another thread.
    fn read_pipe(path: &Path) -> mpsc::Receiver<String> {
        let made = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .unwrap();
        assert!(made.success());
        let (sender, received) = mpsc::channel();
        let path = path.to_owned();
        std::thread::spawn(move || {
            // Opening a pipe for reading waits for its first writer.
            let mut text = String::new();
            std::fs::File::open(path)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            let _ = sender.send(text);
        });
        received
    }

    /// Opens the page of `w1` under `state` with a browser that writes the address it opens
    /// to `address_file`, then exits with `status`.
    fn opener(state: &Path, address_file: &Path, status: u8) -> PageOpener {
        let script = state.join("browser");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf %s \"$1\" > '{}'\nexit {status}\n",
                address_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        PageOpener::new(
            PageDirectory::new(state),
            WorkspaceId("w1".into()),
            Browser::command(&script.display().to_string()),
        )
    }

    fn open() -> Action {
        Action::ExplorePage(ExplorePageAction::Open)
    }

    #[test]
    fn opening_the_page_runs_the_browser_with_its_address() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let address_file = pages.path().join("address_file");
        let addresses = read_pipe(&address_file);
        let opener = opener(pages.path(), &address_file, 0);
        let fixture = EffectsFixture::start(repository_fixture(RepoType::Git), |setup| {
            setup.page_opener = Some(opener);
        });

        fixture.perform([open()]);

        assert_eq!(addresses.recv_timeout(GUARD).unwrap(), host.url());
    }

    #[test]
    fn a_browser_that_fails_tells_the_pane_why_with_the_address() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let address_file = pages.path().join("address_file");
        let opener = opener(pages.path(), &address_file, 3);
        let mut fixture = EffectsFixture::start(repository_fixture(RepoType::Git), |setup| {
            setup.page_opener = Some(opener);
        });

        fixture.perform([open()]);

        let ExplorePageNotOpened(failure) = fixture.wait_for::<ExplorePageNotOpened>();
        assert_eq!(failure.url.as_deref(), Some(host.url()));
        assert!(!failure.reason.is_empty());
    }

    #[test]
    fn without_an_opener_nothing_opens_and_nothing_fails() {
        let mut fixture = EffectsFixture::new(RepoType::Git);

        // Without an opener, opening starts no thread that could fail later: what it says,
        // it says before `perform` returns.
        fixture.perform([open()]);

        assert!(
            fixture
                .drain_events()
                .iter()
                .all(|event| event.downcast_ref::<ExplorePageNotOpened>().is_none())
        );
    }

    /// Network access on the loopback interface, which stands in for a network interface.
    fn loopback() -> NetworkAccess {
        let mut access = NetworkAccess::default();
        access.set_interface(if cfg!(target_os = "linux") {
            "lo"
        } else {
            "lo0"
        });
        access.first_port = 0;
        access
    }

    /// The address of `url`, an address of the page on the network.
    fn socket(url: &str) -> std::net::SocketAddr {
        let rest = url.strip_prefix("http://").unwrap();
        rest[..rest.find('/').unwrap()].parse().unwrap()
    }

    /// Effects that apply the network settings to the page of `host`, with the loopback
    /// interface and any free port saved, and the address the page got there.
    fn sharing(host: &PageHost) -> (EffectsFixture, String) {
        let mut fixture = EffectsFixture::new(RepoType::Git);
        let access = loopback();
        for setting in [
            ExplorePageSetting::Interface(access.interface.clone()),
            ExplorePageSetting::FirstPort(access.first_port),
        ] {
            fixture.store.save_explore_page_setting(setting).unwrap();
        }
        let sharing = PageSharing::new(
            host.network(),
            fixture.background.clone(),
            missing_tunnel(fixture.state.path()),
        );
        sharing.apply(&access);
        let ExplorePageShared(url) = fixture.wait_for::<ExplorePageShared>();
        fixture.effects.share_page(sharing);
        (fixture, url)
    }

    fn save(setting: ExplorePageSetting) -> Action {
        Action::Settings(SettingsAction::SaveExplorePage(setting))
    }

    #[test]
    fn turning_network_access_off_takes_the_page_off_the_network_and_on_brings_it_back() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let (mut fixture, url) = sharing(&host);

        fixture.perform([save(ExplorePageSetting::NetworkEnabled(false))]);

        fixture.wait_for::<ExplorePageOffNetwork>();
        assert!(review_test_support::refuses_connections(socket(&url)));
        let ExplorePageSettingsLoaded(saved) = fixture.wait_for::<ExplorePageSettingsLoaded>();
        assert!(!saved.network.enabled);
        assert_eq!(saved, fixture.store.explore_page_settings().unwrap());

        fixture.perform([save(ExplorePageSetting::NetworkEnabled(true))]);
        let ExplorePageShared(again) = fixture.wait_for::<ExplorePageShared>();
        assert!(std::net::TcpStream::connect(socket(&again)).is_ok());
        assert!(
            fixture
                .store
                .explore_page_settings()
                .unwrap()
                .network
                .enabled
        );
    }

    #[test]
    fn a_change_keeps_the_settings_another_reviewer_saved_and_shows_them() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let (mut fixture, url) = sharing(&host);
        let other = ReviewStore::open(fixture.state.path(), fixture.repository.root()).unwrap();
        other
            .save_explore_page_setting(ExplorePageSetting::NetworkEnabled(false))
            .unwrap();

        fixture.perform([save(ExplorePageSetting::PaneStarts(PaneStarts::InPane))]);

        let ExplorePageSettingsLoaded(saved) = fixture.wait_for::<ExplorePageSettingsLoaded>();
        assert_eq!(saved.pane_starts, PaneStarts::InPane);
        assert!(!saved.network.enabled, "the other reviewer's change stays");
        assert!(review_test_support::refuses_connections(socket(&url)));
    }

    #[test]
    fn an_interface_with_no_address_says_why_and_is_tried_again_when_saved_again() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let (mut fixture, url) = sharing(&host);
        let unknown = ExplorePageSetting::Interface(Some("no-such-interface0".into()));

        fixture.perform([save(unknown.clone())]);

        let ExplorePageNotShared(reason) = fixture.wait_for::<ExplorePageNotShared>();
        assert!(reason.contains("no-such-interface0"), "{reason}");
        assert!(review_test_support::refuses_connections(socket(&url)));
        fixture.perform([save(unknown)]);
        fixture.wait_for::<ExplorePageNotShared>();
    }

    #[test]
    fn a_setting_that_leaves_the_network_as_it_was_keeps_the_page_where_it_is() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let (mut fixture, url) = sharing(&host);

        fixture.perform([save(ExplorePageSetting::PaneStarts(PaneStarts::InPane))]);

        let events = fixture.events_until::<ExplorePageSettingsLoaded>();
        assert!(events.iter().all(|event| {
            event.downcast_ref::<ExplorePageShared>().is_none()
                && event.downcast_ref::<ExplorePageOffNetwork>().is_none()
        }));
        assert!(std::net::TcpStream::connect(socket(&url)).is_ok());
    }

    /// A tunnel whose `cloudflared` is not installed, for tests that open none.
    fn missing_tunnel(state: &Path) -> TunnelProgram {
        let mut tunnel = TunnelProgram::cloudflared(test_fork_tools().wrapper());
        tunnel.program = state.join("no-cloudflared").into_os_string();
        tunnel
    }

    const TUNNEL_HOST: &str = "quiet-river-stone-lamp.trycloudflare.com";

    /// A stand-in for `cloudflared` under `state`, started through the reviewer's own
    /// `fork-exec`: it holds the named pipe `cloudflared.alive` open for writing, writes its
    /// process ID, prints the tunnel's address as `cloudflared` does, and runs until it is
    /// stopped, when the pipe's reader sees its end.
    fn stand_in_tunnel(state: &Path) -> TunnelProgram {
        let program = state.join("cloudflared");
        std::fs::write(
            &program,
            format!(
                "#!/bin/sh\nexec 3>'{}'\necho $$ > '{}'\necho 'INF |  https://{TUNNEL_HOST}  |' >&2\nexec sleep 600\n",
                state.join("cloudflared.alive").display(),
                state.join("cloudflared.pid").display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let mut tunnel = missing_tunnel(state);
        tunnel.program = program.into_os_string();
        tunnel
    }

    /// The tunnel state the pane hears next.
    fn tunnel_state(fixture: &mut EffectsFixture) -> TunnelState {
        let ExplorePageTunnel(state) = fixture.wait_for::<ExplorePageTunnel>();
        state
    }

    #[test]
    fn the_pane_shares_the_running_round_over_a_tunnel_and_stops_it() {
        let pages = tempfile::tempdir().unwrap();
        let round = RoundPublisher::default();
        let commands = CommandSender::new(|_, reply| {
            reply.send(Err(CommandRefusal::Failed(
                "No session in this test".into(),
            )));
        });
        let host = PageHost::start(
            PageRound::new(round.subscribe(), commands),
            &PageDirectory::new(pages.path()),
            &WorkspaceId("w1".into()),
            Path::new("/repositories/drafts"),
        )
        .unwrap();
        let review = review_types::ReviewUnit::from("review");
        let overview = review_explore::RoundOverview {
            rail: Vec::new(),
            decisions: Vec::new(),
            earlier: Vec::new(),
            title: review_explore::TabTitle::AgentWorking,
        };
        round.publish(
            Some(review_explore_page::PublishedRound {
                id: "r1",
                review_unit: &review,
                design: None,
                changed_files: 0,
                cancellable: None,
                earlier: false,
                overview: &overview,
                earlier_citations: &[],
            }),
            review_explore_page::RoundStage::AgentWorking {
                request: "turn".into(),
                sent_at_ms: None,
                answer: None,
            },
        );
        let mut fixture = EffectsFixture::new(RepoType::Git);
        let ended = read_pipe(&pages.path().join("cloudflared.alive"));
        let sharing = PageSharing::new(
            host.network(),
            fixture.background.clone(),
            stand_in_tunnel(pages.path()),
        );
        fixture.effects.share_page(sharing);

        fixture.perform([Action::ExplorePage(ExplorePageAction::OpenTunnel)]);

        assert_eq!(tunnel_state(&mut fixture), TunnelState::Opening);
        let TunnelState::Open { url } = tunnel_state(&mut fixture) else {
            panic!("the tunnel's link");
        };
        assert!(
            url.starts_with(&format!("https://{TUNNEL_HOST}/?token=")),
            "{url}"
        );
        assert!(!url.contains(host.url().rsplit('=').next().unwrap()));
        let pid = std::fs::read_to_string(pages.path().join("cloudflared.pid")).unwrap();
        let process = PathBuf::from(format!("/proc/{}", pid.trim()));
        assert!(process.exists());

        fixture.perform([Action::ExplorePage(ExplorePageAction::CloseTunnel)]);

        assert_eq!(tunnel_state(&mut fixture), TunnelState::Off);
        ended.recv_timeout(GUARD).expect("cloudflared ended");
    }

    #[test]
    fn a_missing_cloudflared_reaches_the_pane_in_one_line() {
        let pages = tempfile::tempdir().unwrap();
        let host = host(pages.path());
        let mut fixture = EffectsFixture::new(RepoType::Git);
        let sharing = PageSharing::new(
            host.network(),
            fixture.background.clone(),
            missing_tunnel(pages.path()),
        );
        fixture.effects.share_page(sharing);

        fixture.perform([Action::ExplorePage(ExplorePageAction::OpenTunnel)]);

        let TunnelState::Failed(reason) = tunnel_state(&mut fixture) else {
            panic!("why there is no tunnel");
        };
        assert!(!reason.contains('\n'), "{reason}");
    }
}
