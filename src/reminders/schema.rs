diesel::table! {
    reminders (id) {
        id -> Integer,
        user_id -> Text,
        title -> Text,
        due_at -> BigInt,
        created_at -> BigInt,
        completed_at -> Nullable<BigInt>,
        fired_at -> Nullable<BigInt>,
    }
}
