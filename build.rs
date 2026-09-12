fn main() {
    // 只有在 Windows 系统下编译时才嵌入图标资源
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap() == "windows" {
        let mut res = winresource::WindowsResource::new();
        // 指向你放在根目录下的 .ico 图标
        res.set_icon("./assets/favicon.ico"); 
        res.compile().unwrap();
    }
}
