use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn collect(root: &Path, dir: &Path, entries: &mut Vec<(String, PathBuf)>) {
    for entry in fs::read_dir(dir).expect("read bundled skills") {
        let path = entry.expect("skill entry").path();
        if path.is_dir() {
            collect(root, &path, entries);
        } else if path.is_file() {
            entries.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                path,
            ));
        }
    }
}
fn main() {
    let root =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("resources/skill-packages");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut entries = Vec::new();
    collect(&root, &root, &mut entries);
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut output = String::from("static BUNDLED_SKILLS: &[(&str, &[u8])] = &[\n");
    for (relative, path) in entries {
        output.push_str(&format!(
            "({relative:?}, include_bytes!({:?})),\n",
            path.to_string_lossy()
        ));
    }
    output.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("bundled_skills.rs"),
        output,
    )
    .unwrap();
}
