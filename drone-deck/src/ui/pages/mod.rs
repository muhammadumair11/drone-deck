//! Management pages. Each takes the zoom-adjusted viewport for its breakpoints.
use super::*;
mod activity;
mod data;
mod settings;
mod system;
mod tools;

pub(super) fn page(s: &State, vp: &Viewport) -> El {
    match s.page {
        Page::System => system::page(s, vp),
        Page::Tools => tools::page(s, vp),
        Page::Data => data::page(s, vp),
        Page::Activity => activity::page(s, vp),
        Page::Settings | Page::Setup => settings::page(s, vp),
    }
}
