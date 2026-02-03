diesel::table! {
    wakeup_tasks (id) {
        id -> Integer,
        user_id -> Text,
        name -> Text,
        prompt -> Text,
        interval_minutes -> BigInt,
        enabled -> Bool,
        created_at -> BigInt,
        updated_at -> BigInt,
        last_run_at -> Nullable<BigInt>,
        next_run_at -> BigInt,
    }
}
