use fission_core::ui::{Button, ButtonVariant, Text, Widget};
use fission_core::{ActionEnvelope, Role, Semantics};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Link {
    pub text: String,
    pub on_click: Option<ActionEnvelope>,
    /// Stable identifier exposed by the link's interactive semantics node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantics_identifier: Option<String>,
}

impl Link {
    /// Sets the stable identifier exposed to accessibility and test tooling.
    pub fn semantics_identifier(mut self, identifier: impl Into<String>) -> Self {
        self.semantics_identifier = Some(identifier.into());
        self
    }
}

impl From<Link> for Widget {
    fn from(component: Link) -> Self {
        let (_, view) = fission_core::build::current::<()>();
        let this = &component;

        let tokens = &view.env().theme.tokens;

        Button {
            variant: ButtonVariant::Ghost,
            child: Some(
                Text::new(this.text.clone())
                    .color(tokens.colors.primary)
                    .underline(true)
                    .into(),
            ),
            on_press: this.on_click.clone(),
            semantics: Some(link_semantics(
                this.semantics_identifier.clone(),
                this.text.clone(),
            )),
            content_align: fission_core::ui::ButtonContentAlign::Start,
            padding: Some([0.0; 4]), // Minimal padding
            ..Default::default()
        }
        .into()
    }
}

fn link_semantics(identifier: Option<String>, label: String) -> Semantics {
    Semantics {
        role: Role::Link,
        label: Some(label),
        identifier,
        focusable: true,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::link_semantics;
    use fission_core::Role;

    #[test]
    fn link_exposes_label_and_stable_identifier() {
        let semantics = link_semantics(Some("docs".into()), "Documentation".into());

        assert_eq!(semantics.role, Role::Link);
        assert_eq!(semantics.identifier.as_deref(), Some("docs"));
        assert_eq!(semantics.label.as_deref(), Some("Documentation"));
        assert!(semantics.focusable);
    }
}
