// 把图标编进 .exe 的资源段——资源管理器、任务栏、Alt+Tab 读的都是这里，
// 运行期用 egui/tray-icon 设的图标管不到 exe 文件本身。
//
// 像素数据与运行期完全共用 src/icon.rs（`include!` 进来），
// 不额外维护一个二进制 .ico 资源文件，改图案只需改一处。

include!("src/icon.rs");

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/icon.rs");

    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR 未设置")).join("quietkey.ico");
    if let Err(e) = write_ico(&out) {
        // 图标不是功能性依赖，构建不该因为它整个失败；但也绝不静默——
        // 打成 cargo warning，编译输出里一眼能看到。
        println!("cargo:warning=生成 quietkey.ico 失败，.exe 将没有图标: {e}");
        return;
    }

    let mut res = winresource::WindowsResource::new();
    res.set_icon(&out.to_string_lossy());
    if let Err(e) = res.compile() {
        println!("cargo:warning=嵌入 .exe 图标失败（是否缺少 Windows SDK 的 rc.exe？）: {e}");
    }
}

/// 写一个多尺寸 .ico。小到托盘 16px、大到资源管理器超大图标 256px 都备一份，
/// 否则 Windows 会拿最近的尺寸硬缩，边缘发糊。
///
/// 编码方式**显式指定**，不用 `IconDirEntry::encode` 的自动启发式：
/// 我们的图标带抗锯齿边缘，会被判定为"复杂 alpha"从而每一档都压成 PNG，
/// 而只有 256×256 那一档是 Vista 之后才普遍支持 PNG 的，
/// 小尺寸走 PNG 在部分 shell 路径下会直接不显示。小尺寸一律用 32 位 BMP。
fn write_ico(path: &PathBuf) -> std::io::Result<()> {
    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in [16u32, 20, 24, 32, 48, 64, 128, 256] {
        let image = ico::IconImage::from_rgba_data(size, size, rgba(size));
        let entry = if size >= 256 {
            ico::IconDirEntry::encode_as_png(&image)?
        } else {
            ico::IconDirEntry::encode_as_bmp(&image)?
        };
        dir.add_entry(entry);
    }
    dir.write(std::fs::File::create(path)?)
}
