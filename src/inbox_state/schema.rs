diesel::table! {
    inbox_item_states (id) {
        id -> Integer,
        user_id -> Text,
        origin_ref -> Text,
        status -> Text,
        created_at -> BigInt,
        updated_at -> BigInt,
    }
}
