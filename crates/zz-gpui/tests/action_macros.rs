use schemars::JsonSchema;
use serde::Deserialize;
use zz_gpui::{Action, actions};
use zz_gpui_macros::register_action;

#[test]
fn test_action_macros() {
    actions!(
        test_only,
        [
            SomeAction,
            /// Documented action
            SomeActionWithDocs,
        ]
    );

    #[derive(PartialEq, Clone, Deserialize, JsonSchema, Action)]
    #[action(namespace = test_only)]
    #[serde(deny_unknown_fields)]
    struct AnotherAction;

    #[derive(PartialEq, Clone, zz_gpui::private::serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RegisterableAction {}

    register_action!(RegisterableAction);

    impl zz_gpui::Action for RegisterableAction {
        fn boxed_clone(&self) -> Box<dyn zz_gpui::Action> {
            unimplemented!()
        }

        fn partial_eq(&self, _action: &dyn zz_gpui::Action) -> bool {
            unimplemented!()
        }

        fn name(&self) -> &'static str {
            unimplemented!()
        }

        fn name_for_type() -> &'static str
        where
            Self: Sized,
        {
            unimplemented!()
        }

        fn build(_value: serde_json::Value) -> anyhow::Result<Box<dyn zz_gpui::Action>>
        where
            Self: Sized,
        {
            unimplemented!()
        }
    }
}
