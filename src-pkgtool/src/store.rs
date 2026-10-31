//! SQLite 持久化层（`packtool.db`，与 exe 同目录）
//!
//! 为什么从「单个 packtool.json」换成 SQLite：
//! - 一个文件承载**多个命名项目**，切换项目不再靠复制/覆盖配置文件；
//! - 写入是事务化的，改到一半断电不会留下半截 JSON；
//! - 后续增加构建历史等数据时不必再换存储。
//!
//! 旧版 `packtool.json` 只读兼容：数据库为空时自动导入为一个命名项目，原文件保留不删。

use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension};

use crate::Config;

/// 数据库文件名（与 exe 同目录）
pub(crate) const DB_FILE: &str = "packtool.db";
/// 旧版单配置文件名（仅用于一次性迁移/降级兜底）
pub(crate) const LEGACY_CONFIG_FILE: &str = "packtool.json";
/// 当前 schema 版本（写入 `PRAGMA user_version`）
const SCHEMA_VERSION: i64 = 1;
/// meta 表里「当前选中项目」的键
const CURRENT_KEY: &str = "current_project_id";

/// 数据库绝对路径（exe 同目录）
pub(crate) fn default_path() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join(DB_FILE)))
        .unwrap_or_else(|| PathBuf::from(DB_FILE))
}

/// 读取旧版 packtool.json（仅迁移/数据库不可用时兜底）
pub(crate) fn load_legacy_config() -> Option<Config> {
    let path = default_path().with_file_name(LEGACY_CONFIG_FILE);
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// 数据库不可用时的内存兜底：优先旧 JSON，其次内置默认值
pub(crate) fn fallback_config() -> Config {
    load_legacy_config().unwrap_or_else(Config::default_config)
}

/// 项目列表项（下拉框只关心 id 与名称）
#[derive(Clone)]
pub(crate) struct ProjectSummary {
    pub(crate) id: i64,
    pub(crate) name: String,
}

/// 首次打开数据库时的初始化结果（调用方据此写启动日志）
pub(crate) enum SeedOutcome {
    /// 已有项目，无需初始化
    Existing,
    /// 从旧版 packtool.json 导入（携带项目名）
    ImportedLegacy(String),
    /// 全新安装，创建默认项目（携带项目名）
    CreatedDefault(String),
}

/// SQLite 连接 + 数据库路径
pub(crate) struct Store {
    conn: Connection,
    path: PathBuf,
}

impl Store {
    pub(crate) fn open(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path)
            .map_err(|e| format!("打开数据库失败（{}）: {e}", path.display()))?;
        conn.busy_timeout(std::time::Duration::from_secs(3))
            .map_err(|e| format!("设置数据库超时失败: {e}"))?;
        let store = Store {
            conn,
            path: path.to_path_buf(),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// 建表 + 版本校验；`user_version` 同时承担「是否已初始化」标记
    fn init_schema(&self) -> Result<(), String> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|e| format!("读取数据库版本失败: {e}"))?;
        if version > SCHEMA_VERSION {
            return Err(format!(
                "数据库版本 {version} 高于本工具支持的 {SCHEMA_VERSION}，请使用新版 pkgtool"
            ));
        }
        if version < 1 {
            self.conn
                .execute_batch(
                    "BEGIN;
                     CREATE TABLE IF NOT EXISTS projects (
                         id INTEGER PRIMARY KEY AUTOINCREMENT,
                         name TEXT NOT NULL COLLATE NOCASE UNIQUE,
                         config_json TEXT NOT NULL,
                         created_at TEXT NOT NULL,
                         updated_at TEXT NOT NULL
                     );
                     CREATE TABLE IF NOT EXISTS meta (
                         key TEXT PRIMARY KEY,
                         value TEXT NOT NULL
                     );
                     PRAGMA user_version = 1;
                     COMMIT;",
                )
                .map_err(|e| format!("初始化数据库表失败: {e}"))?;
        }
        Ok(())
    }

    /// UTC 秒级时间戳（与 version.json 的 release_date 格式一致）
    fn now() -> String {
        chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    /// 首次使用时播种：优先导入旧 JSON，否则创建默认项目
    pub(crate) fn ensure_seeded(&self) -> Result<SeedOutcome, String> {
        if self.count()? > 0 {
            return Ok(SeedOutcome::Existing);
        }
        // 迁移只读取同目录的旧文件，成功后仍保留 packtool.json 作为回退
        let legacy_path = self.path.with_file_name(LEGACY_CONFIG_FILE);
        let legacy = fs::read_to_string(&legacy_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Config>(&text).ok());
        if let Some(cfg) = legacy {
            let name = validate_name(&cfg.project_name_hint())?;
            let id = self.create_project(&name, &cfg)?;
            self.set_current(id)?;
            return Ok(SeedOutcome::ImportedLegacy(name));
        }
        let cfg = Config::default_config();
        let name = "默认项目".to_string();
        let id = self.create_project(&name, &cfg)?;
        self.set_current(id)?;
        Ok(SeedOutcome::CreatedDefault(name))
    }

    /// 取（项目列表, 当前项目 id, 当前配置）；meta 失效时自动回落到第一个项目
    pub(crate) fn bootstrap(&self) -> Result<(Vec<ProjectSummary>, i64, Config), String> {
        let projects = self.list_projects()?;
        let first = projects.first().ok_or("数据库中没有可用项目")?.id;
        let current = match self.current_id()? {
            Some(id) if projects.iter().any(|p| p.id == id) => id,
            _ => {
                self.set_current(first)?;
                first
            }
        };
        let cfg = self
            .load_config(current)?
            .ok_or("当前项目的配置不存在（数据库可能被外部修改）")?;
        Ok((projects, current, cfg))
    }

    pub(crate) fn list_projects(&self) -> Result<Vec<ProjectSummary>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name FROM projects ORDER BY id")
            .map_err(|e| format!("查询项目列表失败: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok(ProjectSummary {
                    id: r.get(0)?,
                    name: r.get(1)?,
                })
            })
            .map_err(|e| format!("查询项目列表失败: {e}"))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| format!("读取项目列表失败: {e}"))?);
        }
        Ok(out)
    }

    /// 新建命名项目；名称已存在时返回可读错误
    pub(crate) fn create_project(&self, name: &str, cfg: &Config) -> Result<i64, String> {
        let name = validate_name(name)?;
        let json = serde_json::to_string(cfg).map_err(|e| format!("序列化项目配置失败: {e}"))?;
        let now = Self::now();
        self.conn
            .execute(
                "INSERT INTO projects (name, config_json, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?3)",
                params![&name, &json, &now],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    format!("项目名「{name}」已存在")
                } else {
                    format!("新建项目失败: {e}")
                }
            })?;
        Ok(self.conn.last_insert_rowid())
    }

    /// 读取项目配置；项目不存在返回 Ok(None)，JSON 损坏返回 Err
    pub(crate) fn load_config(&self, id: i64) -> Result<Option<Config>, String> {
        let json: Option<String> = self
            .conn
            .query_row(
                "SELECT config_json FROM projects WHERE id = ?1",
                [id],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| format!("读取项目配置失败: {e}"))?;
        let Some(json) = json else {
            return Ok(None);
        };
        serde_json::from_str(&json)
            .map(Some)
            .map_err(|e| format!("项目配置损坏（id={id}）: {e}"))
    }

    pub(crate) fn save_config(&self, id: i64, cfg: &Config) -> Result<(), String> {
        let json = serde_json::to_string(cfg).map_err(|e| format!("序列化项目配置失败: {e}"))?;
        let n = self
            .conn
            .execute(
                "UPDATE projects SET config_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![&json, Self::now(), id],
            )
            .map_err(|e| format!("保存项目配置失败: {e}"))?;
        if n == 0 {
            return Err("项目不存在（可能已被删除）".into());
        }
        Ok(())
    }

    /// 重命名并返回规范化后的名称
    pub(crate) fn rename_project(&self, id: i64, name: &str) -> Result<String, String> {
        let name = validate_name(name)?;
        let n = self
            .conn
            .execute(
                "UPDATE projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![&name, Self::now(), id],
            )
            .map_err(|e| {
                if is_unique_violation(&e) {
                    format!("项目名「{name}」已存在")
                } else {
                    format!("重命名项目失败: {e}")
                }
            })?;
        if n == 0 {
            return Err("项目不存在（可能已被删除）".into());
        }
        Ok(name)
    }

    /// 删除项目；至少保留一个，删掉当前项目后自动切到剩余的第一个
    pub(crate) fn delete_project(&self, id: i64) -> Result<(), String> {
        if self.count()? <= 1 {
            return Err("至少需要保留一个项目".into());
        }
        let n = self
            .conn
            .execute("DELETE FROM projects WHERE id = ?1", [id])
            .map_err(|e| format!("删除项目失败: {e}"))?;
        if n == 0 {
            return Err("项目不存在（可能已被删除）".into());
        }
        if self.current_id()? == Some(id) {
            if let Some(next) = self.list_projects()?.first() {
                self.set_current(next.id)?;
            } else {
                self.conn
                    .execute("DELETE FROM meta WHERE key = ?1", [CURRENT_KEY])
                    .map_err(|e| format!("更新当前项目失败: {e}"))?;
            }
        }
        Ok(())
    }

    pub(crate) fn current_id(&self) -> Result<Option<i64>, String> {
        let value: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM meta WHERE key = ?1",
                [CURRENT_KEY],
                |r| r.get(0),
            )
            .optional()
            .map_err(|e| format!("读取当前项目失败: {e}"))?;
        Ok(value.and_then(|v| v.parse().ok()))
    }

    pub(crate) fn set_current(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO meta (key, value) VALUES (?1, ?2) \
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![CURRENT_KEY, id.to_string()],
            )
            .map_err(|e| format!("保存当前项目失败: {e}"))?;
        Ok(())
    }

    fn count(&self) -> Result<i64, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .map_err(|e| format!("读取项目数量失败: {e}"))
    }
}

/// 项目名校验：去首尾空白、非空、≤64 字符、不含控制字符
pub(crate) fn validate_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("项目名不能为空".into());
    }
    if name.chars().count() > 64 {
        return Err("项目名最多 64 个字符".into());
    }
    if name.chars().any(char::is_control) {
        return Err("项目名不能包含换行等控制字符".into());
    }
    Ok(name.to_string())
}

/// UNIQUE 约束冲突 → 项目名重复（表上只有 name 一个唯一约束）
fn is_unique_violation(e: &rusqlite::Error) -> bool {
    matches!(
        e,
        rusqlite::Error::SqliteFailure(err, _)
            if err.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpdir(tag: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("pkgtool_store_test_{}_{}", tag, std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn cfg(app: &str) -> Config {
        Config {
            app_name: app.into(),
            files: vec![format!("D:\\src\\{app}.exe")],
            download_prefixes: vec![format!("https://example.com/{app}/")],
            volume_size_mb: 7,
            output_dir: format!("D:\\out\\{app}"),
            min_version: String::new(),
            force_update: false,
            store_url: String::new(),
        }
    }

    #[test]
    fn imports_legacy_json_as_named_project() {
        let dir = tmpdir("legacy");
        let legacy = cfg("oldapp");
        fs::write(
            dir.join(LEGACY_CONFIG_FILE),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let store = Store::open(&dir.join(DB_FILE)).unwrap();
        let outcome = store.ensure_seeded().unwrap();
        assert!(
            matches!(&outcome, SeedOutcome::ImportedLegacy(n) if n == "oldapp"),
            "旧 JSON 应导入为同名项目"
        );

        let (projects, current, loaded) = store.bootstrap().unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "oldapp");
        assert_eq!(current, projects[0].id);
        assert_eq!(loaded.app_name, "oldapp");
        assert_eq!(loaded.volume_size_mb, 7);
        assert_eq!(loaded.files, vec!["D:\\src\\oldapp.exe".to_string()]);
        // 迁移不写回、不删除旧文件
        assert!(dir.join(LEGACY_CONFIG_FILE).is_file());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn creates_switches_and_persists_projects() {
        let dir = tmpdir("multi");
        let db = dir.join(DB_FILE);
        let store = Store::open(&db).unwrap();
        assert!(matches!(
            store.ensure_seeded().unwrap(),
            SeedOutcome::CreatedDefault(n) if n == "默认项目"
        ));

        let b_id = store.create_project("项目 B", &cfg("appb")).unwrap();
        let c_id = store.create_project("项目 C", &cfg("appc")).unwrap();
        store.set_current(b_id).unwrap();
        drop(store);

        // 重开：项目、当前选择、配置都应从 SQLite 恢复
        let store = Store::open(&db).unwrap();
        assert!(matches!(
            store.ensure_seeded().unwrap(),
            SeedOutcome::Existing
        ));
        let (projects, current, loaded) = store.bootstrap().unwrap();
        let names: Vec<&str> = projects.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, vec!["默认项目", "项目 B", "项目 C"]);
        assert_eq!(current, b_id);
        assert_eq!(loaded.app_name, "appb");

        // 修改配置并落盘，再读回
        let mut edited = loaded;
        edited.volume_size_mb = 42;
        edited.files.push("extra.txt".into());
        store.save_config(c_id, &edited).unwrap();
        let reloaded = store.load_config(c_id).unwrap().unwrap();
        assert_eq!(reloaded.volume_size_mb, 42);
        assert_eq!(reloaded.files.last().unwrap(), "extra.txt");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn rename_delete_and_name_rules() {
        let dir = tmpdir("rename");
        let store = Store::open(&dir.join(DB_FILE)).unwrap();
        store.ensure_seeded().unwrap();
        let first = store.bootstrap().unwrap().1;
        let second = store.create_project("second", &cfg("s")).unwrap();

        // 唯一性（大小写不敏感，trim 后比较）
        let err = store.rename_project(second, " 默认项目 ").unwrap_err();
        assert!(err.contains("已存在"), "应提示重名: {err}");
        let err = store.create_project("SECOND", &cfg("s2")).unwrap_err();
        assert!(err.contains("已存在"), "大小写不同也应视为重名: {err}");
        assert_eq!(
            store.rename_project(second, "  发布版  ").unwrap(),
            "发布版"
        );

        // 名称规则
        assert!(validate_name("   ").is_err());
        assert!(validate_name("a\nb").is_err());
        assert!(validate_name(&"x".repeat(65)).is_err());
        assert_eq!(validate_name(" ok ").unwrap(), "ok");

        // 删除当前项目后自动回落；最后一个不允许删
        store.set_current(second).unwrap();
        store.delete_project(second).unwrap();
        assert_eq!(store.current_id().unwrap(), Some(first));
        let err = store.delete_project(first).unwrap_err();
        assert!(err.contains("至少"), "应拒绝删除最后一个项目: {err}");
        let _ = fs::remove_dir_all(&dir);
    }
}
