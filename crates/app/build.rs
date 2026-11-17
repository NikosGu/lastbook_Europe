//! Windows: the program's icon and its name in the file's properties.
fn main() {
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../packaging/icon/Lastbook.ico");
        res.set("ProductName", "Lastbook Europe");
        res.set("FileDescription", "Lastbook Europe");
        res.compile().expect("the Windows resources could not be built");
    }
}
