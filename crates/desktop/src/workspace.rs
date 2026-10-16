use crate::layout::{Axis, Layout, PaneId};
use whostty_core::search::Search;
use whostty_core::{config::Profile, session::Session};

pub struct Pane {
    pub id: PaneId,
    pub profile: Profile,
    pub title: String,
    pub session: Option<Session>,
    pub error: Option<String>,
    pub exit: Option<u32>,
    pub bell: bool,
    pub view: crate::view::ViewState,
    pub search: Search,
}

impl Pane {
    pub fn running(&self) -> bool {
        self.session.is_some() && self.exit.is_none()
    }
}

pub struct Tab {
    pub panes: Vec<Pane>,
    pub layout: Layout,
    pub active: PaneId,
}

impl Tab {
    pub fn new(pane: Pane) -> Self {
        let id = pane.id;
        Self {
            panes: vec![pane],
            layout: Layout::Pane(id),
            active: id,
        }
    }
    pub fn pane(&self) -> &Pane {
        self.panes
            .iter()
            .find(|pane| pane.id == self.active)
            .expect("Active pane must exist")
    }
    pub fn pane_mut(&mut self) -> &mut Pane {
        self.panes
            .iter_mut()
            .find(|pane| pane.id == self.active)
            .expect("Active pane must exist")
    }
    pub fn split(&mut self, pane: Pane, axis: Axis) {
        let id = pane.id;
        assert!(self.layout.split(self.active, id, axis));
        self.panes.push(pane);
        self.active = id;
    }
    pub fn close_pane(&mut self, id: PaneId) -> bool {
        if self.panes.len() == 1 || !self.layout.remove(id) {
            return false;
        }
        self.panes.retain(|pane| pane.id != id);
        if self.active == id {
            self.active = self.layout.ids()[0];
        }
        true
    }
    pub fn running(&self) -> bool {
        self.panes.iter().any(Pane::running)
    }
    pub fn bell(&self) -> bool {
        self.panes.iter().any(|pane| pane.bell)
    }
}
