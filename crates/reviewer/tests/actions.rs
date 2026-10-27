use std::sync::Mutex;

use herdr_client::protocol::{
    EntrypointId, HerdrReader, HerdrWriter, OpenPluginPane, PaneId, PanePlacement, PaneSize,
    PluginContext, PluginPane, SplitDirection, TabId, WorkspaceId, method,
};
use herdr_client::{Error, Result};
use review_repository::repository::RepoType;
use review_test_support::repository_fixture;
use reviewer::control::{PaneAction, PaneActionResult, PaneActions};
use test_case::test_case;

#[derive(Debug, Default)]
struct FakeHerdr {
    panes: Mutex<Vec<PluginPane>>,
    opened: Mutex<Vec<OpenPluginPane>>,
    focused: Mutex<Vec<PaneId>>,
    closed: Mutex<Vec<PaneId>>,
    race_on_open: Mutex<bool>,
    /// The focused pane's size, or `None` when Herdr cannot measure it.
    focused_size: Mutex<Option<PaneSize>>,
}

impl HerdrReader for FakeHerdr {
    fn read_agent_screen(&self, _pane_id: &PaneId) -> Result<String> {
        unreachable!()
    }

    fn list_plugin_panes(&self, tab_id: &TabId) -> Result<Vec<PluginPane>> {
        Ok(self
            .panes
            .lock()
            .unwrap()
            .iter()
            .filter(|pane| pane.tab_id == *tab_id)
            .cloned()
            .collect())
    }

    fn pane_size(&self, _pane_id: &PaneId) -> Result<PaneSize> {
        self.focused_size
            .lock()
            .unwrap()
            .ok_or_else(|| Error::Protocol {
                operation: method::PANE_LAYOUT.to_owned(),
                detail: "unmeasured",
            })
    }
}

impl HerdrWriter for FakeHerdr {
    fn open_plugin_pane(&self, request: &OpenPluginPane) -> Result<PluginPane> {
        self.opened.lock().unwrap().push(request.clone());
        let mut panes = self.panes.lock().unwrap();
        if std::mem::take(&mut *self.race_on_open.lock().unwrap()) {
            panes.push(review_pane("review-first"));
        }
        let pane = review_pane("review-new");
        panes.push(pane.clone());
        Ok(pane)
    }

    fn focus_plugin_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.focused.lock().unwrap().push(pane_id.clone());
        Ok(())
    }

    fn focus_agent(&self, pane_id: &PaneId) -> Result<()> {
        self.focused.lock().unwrap().push(pane_id.clone());
        Ok(())
    }

    fn close_plugin_pane(&self, pane_id: &PaneId) -> Result<()> {
        self.closed.lock().unwrap().push(pane_id.clone());
        self.panes
            .lock()
            .unwrap()
            .retain(|pane| pane.pane_id != *pane_id);
        Ok(())
    }
}

#[test_case(RepoType::Git; "git")]
#[test_case(RepoType::Jj; "jj")]
fn pane_actions_are_idempotent_and_remove_a_racing_duplicate(repository_type: RepoType) {
    let repository = repository_fixture(repository_type);
    let client = FakeHerdr::default();
    *client.race_on_open.lock().unwrap() = true;
    let context = tab_context(repository.root());
    let actions = PaneActions::new(&client);

    assert_eq!(
        actions.run(PaneAction::Open, &context).unwrap(),
        PaneActionResult::Focused(PaneId("review-first".to_owned()))
    );
    assert_eq!(
        client.opened.lock().unwrap().as_slice(),
        [OpenPluginPane {
            entrypoint: EntrypointId("review".to_owned()),
            placement: PanePlacement::Split,
            target_pane_id: PaneId("agent".to_owned()),
            direction: None,
            cwd: repository.root().to_owned(),
            focus: true,
        }]
    );
    assert_eq!(
        client.closed.lock().unwrap().as_slice(),
        [PaneId("review-new".to_owned())]
    );
    assert_eq!(
        actions.run(PaneAction::Open, &context).unwrap(),
        PaneActionResult::Focused(PaneId("review-first".to_owned()))
    );
    assert_eq!(client.opened.lock().unwrap().len(), 1);

    assert_eq!(
        actions.run(PaneAction::Toggle, &context).unwrap(),
        PaneActionResult::Closed
    );
    assert_eq!(
        actions.run(PaneAction::Close, &context).unwrap(),
        PaneActionResult::AlreadyClosed
    );
}

#[test_case(Some((240, 60)), Some(SplitDirection::Right); "a wide pane splits beside")]
#[test_case(Some((80, 58)), Some(SplitDirection::Down); "a tall pane splits below")]
#[test_case(None, None; "an unmeasured pane splits where Herdr chooses")]
fn the_review_pane_splits_the_focused_pane_across_its_longer_side(
    focused: Option<(u16, u16)>,
    direction: Option<SplitDirection>,
) {
    let repository = repository_fixture(RepoType::Git);
    let client = FakeHerdr::default();
    *client.focused_size.lock().unwrap() =
        focused.map(|(width, height)| PaneSize { width, height });
    let context = tab_context(repository.root());

    PaneActions::new(&client)
        .run(PaneAction::Open, &context)
        .unwrap();

    assert_eq!(client.opened.lock().unwrap()[0].direction, direction);
}

// herdr-bowtie gives the lead and each task a tab of one workspace, each with its own
// reviewer: an action acts on the review pane of its own tab only.
#[test_case(PaneAction::Open, &PaneActionResult::Opened(PaneId("review-new".to_owned())); "open")]
#[test_case(PaneAction::Toggle, &PaneActionResult::Opened(PaneId("review-new".to_owned())); "toggle")]
#[test_case(PaneAction::Close, &PaneActionResult::AlreadyClosed; "close")]
fn an_action_keeps_the_review_pane_of_another_tab(action: PaneAction, expected: &PaneActionResult) {
    let repository = repository_fixture(RepoType::Git);
    let client = FakeHerdr::default();
    client
        .panes
        .lock()
        .unwrap()
        .push(review_pane_in("review-elsewhere", "other-tab"));

    assert_eq!(
        &PaneActions::new(&client)
            .run(action, &tab_context(repository.root()))
            .unwrap(),
        expected
    );
    assert_eq!(client.closed.lock().unwrap().as_slice(), []);
}

fn tab_context(cwd: &std::path::Path) -> PluginContext {
    PluginContext {
        workspace_id: Some(WorkspaceId("workspace".to_owned())),
        tab_id: Some(TabId("tab".to_owned())),
        focused_pane_id: Some(PaneId("agent".to_owned())),
        focused_pane_cwd: Some(cwd.to_owned()),
    }
}

fn review_pane(id: &str) -> PluginPane {
    review_pane_in(id, "tab")
}

fn review_pane_in(id: &str, tab: &str) -> PluginPane {
    PluginPane {
        pane_id: PaneId(id.to_owned()),
        tab_id: TabId(tab.to_owned()),
        workspace_id: WorkspaceId("workspace".to_owned()),
        entrypoint_id: EntrypointId("review".to_owned()),
    }
}
