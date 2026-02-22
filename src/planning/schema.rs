diesel::table! {
    plans (id) {
        id -> Integer,
        user_id -> Text,
        title -> Text,
        goal -> Text,
        steps_json -> Nullable<Text>,
        status -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}

diesel::table! {
    plan_step_dependencies (id) {
        id -> Integer,
        plan_id -> Integer,
        user_id -> Text,
        step_ref -> Text,
        depends_on_ref -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}
