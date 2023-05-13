#[macro_export]
macro_rules! link {
    (ram, $fn_name:ident) => {
        concat!(".ram.", file!(), ".", line!(), ".", stringify!($fn_name))
    };
    (ram small 0, $fn_name:ident) => {
        concat!(
            ".small.0.",
            file!(),
            ".",
            line!(),
            ".",
            stringify!($fn_name)
        )
    };
    (ram small 1, $fn_name:ident) => {
        concat!(
            ".small.1.",
            file!(),
            ".",
            line!(),
            ".",
            stringify!($fn_name)
        )
    };
}
