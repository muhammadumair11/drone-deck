//! Setup recipes from `scripts/setup/catalog.json` and the launch profile each one configures.
use serde::Deserialize;

#[derive(Clone, Deserialize)]
pub(crate) struct Recipe {
    pub id: String,
    pub name: String,
    pub version: String,
    pub target: String,
    pub docs: String,
    pub summary: String,
    pub steps: Vec<String>,
    pub automatic: bool,
    #[serde(default)]
    pub destination: String,
    #[serde(default)]
    pub privileges: String,
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub verification: String,
    /// The launch profile this recipe configures, when its id differs from the recipe's.
    #[serde(default)]
    profile: String,
    /// How an existing installation is chosen: `wsl-folder`, `wsl-file`, `windows-folder` or
    /// `windows-file` (read by `picker.ps1`). Empty when there is nothing to choose.
    #[serde(default)]
    pub pick: String,
    /// Recipes this one builds on (jMAVSim lives in the PX4 checkout).
    #[serde(default)]
    pub depends: Vec<String>,
    /// Steps for a tool the user installs themselves (paid, licensed, or with its own
    /// installer). A recipe with a guide is never run as a job.
    #[serde(default)]
    pub guide: Vec<GuideStep>,
}
#[derive(Clone, Deserialize)]
pub(crate) struct GuideStep {
    pub title: String,
    pub body: String,
    #[serde(default)]
    pub link: String,
}
impl Recipe {
    /// Installed by the user from its guide, then checked; never a job.
    pub(crate) fn guided(&self) -> bool {
        !self.automatic && !self.guide.is_empty()
    }
    /// The launch profile this recipe configures.
    pub(crate) fn profile(&self) -> &str {
        if self.profile.is_empty() {
            &self.id
        } else {
            &self.profile
        }
    }
}

/// The recipes compiled into the app.
pub(crate) fn load() -> Vec<Recipe> {
    serde_json::from_str(include_str!("../../scripts/setup/catalog.json")).unwrap()
}

/// The launch profile that recipe `id` configures (`id` itself for an unknown recipe).
pub(crate) fn profile_for<'a>(recipes: &'a [Recipe], id: &'a str) -> &'a str {
    recipes
        .iter()
        .find(|r| r.id == id)
        .map_or(id, Recipe::profile)
}

/// The recipe that configures launch profile `id` (`id` itself when none names it).
pub(crate) fn recipe_for<'a>(recipes: &'a [Recipe], id: &'a str) -> &'a str {
    recipes
        .iter()
        .find(|r| r.profile() == id)
        .map_or(id, |r| r.id.as_str())
}
