use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    for path in ["app.manifest", "build.rs", "appstack.svg"] {
        println!("cargo:rerun-if-changed={path}");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    if env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bin=AppStackClassic=delayimp.lib");
        for library in [
            "combase", "ole32", "oleaut32", "advapi32", "dwmapi", "comctl32", "uxtheme", "shell32",
        ] {
            println!("cargo:rustc-link-arg-bin=AppStackClassic=/DELAYLOAD:{library}.dll");
        }
    }
    let output = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    for (binary, source, variable) in [(
        "AppStackClassic",
        "appstack.svg",
        "APPSTACK_CLASSIC_LOGO_REVISION",
    )] {
        let bytes = fs::read(source).expect("read application SVG");
        let revision = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
            (hash ^ *byte as u64).wrapping_mul(0x100000001b3)
        });
        println!("cargo:rustc-env={variable}={revision:016x}");
        let mut options = resvg::usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        let tree = resvg::usvg::Tree::from_data(&bytes, &options).expect("parse application SVG");
        let mut directory = ico::IconDir::new(ico::ResourceType::Icon);
        for size in [16u32, 20, 24, 32, 40, 48, 64, 128, 256] {
            let mut pixels =
                resvg::tiny_skia::Pixmap::new(size, size).expect("allocate icon bitmap");
            let transform = resvg::tiny_skia::Transform::from_scale(
                size as f32 / tree.size().width(),
                size as f32 / tree.size().height(),
            );
            resvg::render(&tree, transform, &mut pixels.as_mut());
            let mut rgba = pixels.data().to_vec();
            for pixel in rgba.chunks_exact_mut(4) {
                if pixel[3] != 0 && pixel[3] != 255 {
                    let alpha = pixel[3] as u32;
                    for channel in &mut pixel[..3] {
                        *channel = (*channel as u32 * 255 / alpha).min(255) as u8;
                    }
                }
            }
            directory.add_entry(
                ico::IconDirEntry::encode(&ico::IconImage::from_rgba_data(size, size, rgba))
                    .expect("encode application icon"),
            );
        }
        let icon = output.join(format!("{binary}.ico"));
        directory
            .write(fs::File::create(&icon).expect("create application icon"))
            .expect("write application icon");
        let manifest = Path::new("app.manifest").canonicalize().unwrap();
        let resource = output.join(format!("{binary}.rc"));
        let quote = |path: &Path| {
            path.to_string_lossy()
                .replace('\\', "/")
                .replace('"', "\\\"")
        };
        fs::write(
            &resource,
            format!(
                r#"
#pragma code_page(65001)
1 ICON "{}"
1 24 "{}"
1 VERSIONINFO
FILEVERSION 1,0,0,0
PRODUCTVERSION 1,0,0,0
FILEOS 0x40004
FILETYPE 1
BEGIN
  BLOCK "StringFileInfo" BEGIN BLOCK "040904b0" BEGIN
    VALUE "ProductName", "{binary}\0"
    VALUE "FileDescription", "{binary} - Application groups\0"
    VALUE "OriginalFilename", "{binary}.exe\0"
    VALUE "FileVersion", "1.0.0\0"
    VALUE "ProductVersion", "1.0.0\0"
  END END
  BLOCK "VarFileInfo" BEGIN VALUE "Translation", 0x409, 1200 END
END
"#,
                quote(&icon),
                quote(&manifest)
            ),
        )
        .expect("write binary resources");
        embed_resource::compile_for(&resource, [binary], embed_resource::NONE)
            .manifest_required()
            .expect("compile binary resources");
    }
}
