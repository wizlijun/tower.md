//! Launch the matching packaged App through LaunchServices. Never start the
//! native GUI executable directly or request another instance with `open -n`.
use std::path::{Path,PathBuf};
use serde_json::{json,Value};

const APP_CONFIG:&str=include_str!("../../../src-tauri/tauri.conf.json");
struct LaunchPlan { bundle:PathBuf }

fn bundle_for_sidecar(executable:&Path)->Option<PathBuf> {
    let macos=executable.parent()?;
    let contents=macos.parent()?;
    let bundle=contents.parent()?;
    (macos.file_name()?=="MacOS" && contents.file_name()?=="Contents" && bundle.extension()?=="app")
        .then(||bundle.to_owned())
}
fn validate_bundle(bundle:&Path,expected_id:&str,read_info:&mut impl FnMut(&Path)->Result<Value,String>)->Result<LaunchPlan,String> {
    let bundle=bundle.canonicalize().map_err(|error|format!("App 不存在 {}：{error}",bundle.display()))?;
    let info=bundle.join("Contents/Info.plist");
    if !std::fs::symlink_metadata(&info).map_err(|error|error.to_string())?.file_type().is_file() {
        return Err("App 的 Info.plist 不是普通文件".into());
    }
    if !info.canonicalize().map_err(|error|error.to_string())?.starts_with(&bundle) {return Err("App 元数据越出发行包".into());}
    let metadata=read_info(&info)?;
    if metadata["CFBundleIdentifier"].as_str()!=Some(expected_id) {return Err("App 标识与当前 towermd 发行包不匹配".into());}
    let name=metadata["CFBundleExecutable"].as_str().ok_or("App 缺少主程序名")?;
    if name.is_empty() || name=="towermd" || name=="." || name==".." || name.contains(['/', '\\']) || name.chars().any(char::is_control) {
        return Err("App 主程序名无效".into());
    }
    let binary=bundle.join("Contents/MacOS").join(name);
    if !std::fs::symlink_metadata(&binary).map_err(|error|format!("App 主程序不存在：{error}"))?.file_type().is_file() {
        return Err("App 主程序必须是同一发行包内的普通文件".into());
    }
    if !binary.canonicalize().map_err(|error|error.to_string())?.starts_with(&bundle) {return Err("App 主程序越出发行包".into());}
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&binary).map_err(|error|error.to_string())?.permissions().mode() & 0o111==0 {
            return Err("App 主程序没有执行权限".into());
        }
    }
    Ok(LaunchPlan{bundle})
}
fn plan(executable:&Path,workspace:&Path,mut read_info:impl FnMut(&Path)->Result<Value,String>)->Result<LaunchPlan,String> {
    let executable=executable.canonicalize().map_err(|error|format!("无法定位当前 towermd：{error}"))?;
    let config:Value=serde_json::from_str(APP_CONFIG).map_err(|error|error.to_string())?;
    let expected_id=config["identifier"].as_str().ok_or("构建配置缺少 App 标识")?;
    if let Some(bundle)=bundle_for_sidecar(&executable) {
        // A broken/mismatched packaged App must not silently launch another
        // installation from a development checkout.
        return validate_bundle(&bundle,expected_id,&mut read_info);
    }
    let name=format!("{}.app",config["productName"].as_str().ok_or("构建配置缺少 App 名称")?);
    let parent=executable.parent().ok_or("当前 towermd 没有父目录")?;
    let candidates=[parent.join("bundle/macos").join(&name),workspace.join("src-tauri/target/release/bundle/macos").join(&name)];
    for candidate in &candidates {
        if candidate.exists() {return validate_bundle(candidate,expected_id,&mut read_info)
            .map_err(|error|format!("{error}；请在 {} 重新运行 pnpm app:build，核对 {}",workspace.display(),candidate.display()));}
    }
    Err(format!("未找到当前构建的 App。请在 {} 运行 pnpm app:build，生成 {}；若使用自定义 Cargo target，App 应位于 {}。也可直接使用发行包 Contents/MacOS/towermd up。尚未启动守护进程。",
        workspace.display(),candidates[1].display(),candidates[0].display()))
}
fn execute(plan:LaunchPlan,ensure:impl FnOnce()->Result<Value,String>,launch:impl FnOnce(&Path)->Result<(),String>)->Result<Value,String> {
    let daemon=ensure()?;
    launch(&plan.bundle).map_err(|error|format!("守护进程已就绪，但 App 启动未确认：{error}。启动结果未知，请先查看 App 状态"))?;
    Ok(json!({"status":"launch_requested","app":plan.bundle,"daemon_pid":daemon["pid"]}))
}

#[cfg(target_os="macos")]
fn system_output(program:&str,args:&[&std::ffi::OsStr])->Result<Vec<u8>,String> {
    use std::{io::Read,process::{Command,Stdio},sync::mpsc,time::{Duration,Instant}};
    let mut child=Command::new(program).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
        .spawn().map_err(|error|format!("无法运行 {program}：{error}"))?;
    let output=child.stdout.take().ok_or("无法读取系统命令输出")?;
    let (sender,receiver)=mpsc::channel();
    std::thread::spawn(move|| {
        let mut bytes=Vec::new();
        let result=output.take(128*1024+1).read_to_end(&mut bytes).map(|_|bytes).map_err(|error|error.to_string());
        let _=sender.send(result);
    });
    let deadline=Instant::now()+Duration::from_secs(15);
    let status=loop {
        match child.try_wait() {
            Ok(Some(status))=>break status,
            Ok(None) if Instant::now()<deadline=>std::thread::sleep(Duration::from_millis(25)),
            other=>{
                let _=child.kill();let _=child.wait();
                return Err(match other {Err(error)=>error.to_string(),_=>format!("{program} 超时，请检查系统状态；启动结果未知")});
            },
        }
    };
    if !status.success() {return Err(format!("{program} 返回 {status}"));}
    let bytes=receiver.recv_timeout(Duration::from_secs(1)).map_err(|_|"系统命令输出未结束")??;
    if bytes.len()>128*1024 {return Err("系统命令输出超过限制".into());}
    Ok(bytes)
}

pub fn up()->Result<Value,String> {
    #[cfg(target_os="macos")] {
        use std::ffi::OsStr;
        let executable=std::env::current_exe().map_err(|error|error.to_string())?;
        let workspace=Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let launch_plan=plan(&executable,&workspace,|info| {
            let bytes=system_output("/usr/bin/plutil",&[OsStr::new("-convert"),OsStr::new("json"),OsStr::new("-o"),OsStr::new("-"),info.as_os_str()])?;
            serde_json::from_slice(&bytes).map_err(|error|format!("无法读取 App 元数据：{error}"))
        })?;
        execute(launch_plan,crate::daemon::ensure,|bundle| {
            // Default LaunchServices behavior activates the existing instance.
            system_output("/usr/bin/open",&[OsStr::new("-a"),bundle.as_os_str()]).map(|_|())
        })
    }
    #[cfg(not(target_os="macos"))] {
        Err("当前发行包的 towermd up 仅支持 macOS；本平台尚无可启动的 App，守护进程未启动".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell,fs};
    fn binary(path:&Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(path,b"fixture, never execute").unwrap();
        #[cfg(unix)] {use std::os::unix::fs::PermissionsExt;fs::set_permissions(path,fs::Permissions::from_mode(0o755)).unwrap();}
    }
    fn bundle(root:&Path)->PathBuf {
        let bundle=root.join("Tower.md.app");
        binary(&bundle.join("Contents/MacOS/towermd-app"));
        binary(&bundle.join("Contents/MacOS/towermd"));
        fs::write(bundle.join("Contents/Info.plist"),b"fixture read by injected parser").unwrap();bundle
    }
    fn info(_: &Path)->Result<Value,String> {
        let config:Value=serde_json::from_str(APP_CONFIG).unwrap();
        Ok(json!({"CFBundleIdentifier":config["identifier"],"CFBundleExecutable":"towermd-app"}))
    }
    #[test]
    fn packaged_sidecar_uses_its_own_bundle_and_launch_order_is_daemon_then_app() {
        let dir=tempfile::tempdir().unwrap();let app=bundle(dir.path());
        let plan=plan(&app.join("Contents/MacOS/towermd"),Path::new("/unrelated"),info).unwrap();
        assert_eq!(plan.bundle,app.canonicalize().unwrap());
        let calls=RefCell::new(Vec::new());
        let result=execute(plan,||{calls.borrow_mut().push("daemon");Ok(json!({"pid":123,"instance":"private-token"}))},|actual|{
            assert_eq!(actual,app.canonicalize().unwrap());calls.borrow_mut().push("app");Ok(())
        }).unwrap();
        assert_eq!(*calls.borrow(),vec!["daemon","app"]);assert_eq!(result["status"],"launch_requested");
        assert_eq!(result["daemon_pid"],123);assert!(!result.to_string().contains("private-token"));
    }
    #[test]
    fn missing_development_app_reports_build_location_and_existing_target_bundle_is_found() {
        let dir=tempfile::tempdir().unwrap();let cli=dir.path().join("custom-target/release/towermd");binary(&cli);
        let error=plan(&cli,dir.path(),info).err().unwrap();
        assert!(error.contains("pnpm app:build"));assert!(error.contains("src-tauri/target/release/bundle/macos"));
        let app=bundle(&dir.path().join("custom-target/release/bundle/macos"));
        assert_eq!(plan(&cli,dir.path(),info).unwrap().bundle,app.canonicalize().unwrap());
    }
    #[test]
    fn broken_packaged_app_never_falls_back_to_another_installation() {
        let dir=tempfile::tempdir().unwrap();let app=bundle(dir.path());
        bundle(&dir.path().join("src-tauri/target/release/bundle/macos"));
        assert!(plan(&app.join("Contents/MacOS/towermd"),dir.path(),|_|Ok(json!({"CFBundleIdentifier":"other.app","CFBundleExecutable":"towermd-app"}))).is_err());
        assert!(plan(&app.join("Contents/MacOS/towermd"),dir.path(),|_|{
            let mut value=info(Path::new("unused"))?;value["CFBundleExecutable"]=json!("../../elsewhere");Ok(value)
        }).is_err());
        fs::remove_file(app.join("Contents/MacOS/towermd-app")).unwrap();
        assert!(plan(&app.join("Contents/MacOS/towermd"),dir.path(),info).is_err());
    }
    #[test]
    fn daemon_and_launcher_failures_are_not_reported_as_app_running() {
        let dir=tempfile::tempdir().unwrap();let app=bundle(dir.path());
        let choose=||plan(&app.join("Contents/MacOS/towermd"),dir.path(),info).unwrap();
        assert!(execute(choose(),||Err("daemon failed".into()),|_|panic!("must not launch without daemon")).is_err());
        assert!(execute(choose(),||Ok(json!({"pid":1})),|_|Err("open failed".into())).unwrap_err().contains("启动未确认"));
    }
}
