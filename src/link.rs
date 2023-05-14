#[macro_export]
macro_rules! link {
    (ram, $fn_name:ident) => {
        concat!(".ram.", file!(), ".", line!(), ".", stringify!($fn_name))
    };
    (scratch x, $fn_name:ident) => {
        concat!(
            ".scratch_x.",
            file!(),
            ".",
            line!(),
            ".",
            stringify!($fn_name)
        )
    };
    (scratch y, $fn_name:ident) => {
        concat!(
            ".scratch_y.",
            file!(),
            ".",
            line!(),
            ".",
            stringify!($fn_name)
        )
    };
}
