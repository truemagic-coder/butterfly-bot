diesel::table! {
    todo_items (id) {
        id -> Integer,
        user_id -> Text,
        title -> Text,
        notes -> Nullable<Text>,
        position -> Integer,
        created_at -> BigInt,
        updated_at -> BigInt,
        completed_at -> Nullable<BigInt>,
        t_shirt_size -> Nullable<Text>,
        story_points -> Nullable<Integer>,
        estimate_optimistic_minutes -> Nullable<Integer>,
        estimate_likely_minutes -> Nullable<Integer>,
        estimate_pessimistic_minutes -> Nullable<Integer>,
        dependency_refs -> Nullable<Text>,
    }
}
