use crate::daemon_server::config::RuntimeConfig;
use crate::daemon_server::db::*;

impl DbManager {
    pub fn load_runtime_config(&self) -> Result<Option<RuntimeConfig>, DbError> {
        let remote_addr = self.get_config("remote_addr")?;
        let editor = self.get_config("editor")?;
        let default_user = self.get_config("default_user")?;

        if remote_addr.is_none() || editor.is_none() {
            return Ok(None);
        }

        Ok(Some(RuntimeConfig {
            remote_addr: remote_addr.unwrap(),
            editor: editor.unwrap(),
            default_user: default_user.unwrap(),
        }))
    }

    /// 获取应用配置 (反序列化示例)
    fn get_config(&self, key: &str) -> Result<Option<String>, DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_APP_CONFIG)
            .expect(&format!("cf {} must exist", Self::CF_APP_CONFIG));

        match self.inner.get_cf(cf, key)? {
            Some(bytes) => {
                // 假设配置存的是 UTF-8 字符串，如果用 Protobuf，这里用 prost 解码
                let val = String::from_utf8_lossy(&bytes).to_string();
                Ok(Some(val))
            }
            None => Ok(None),
        }
    }

    /// 写入应用配置
    pub fn set_config(&self, key: &str, value: &str) -> Result<(), DbError> {
        let cf = self
            .inner
            .cf_handle(Self::CF_APP_CONFIG)
            .expect(&format!("cf {} must exist", Self::CF_APP_CONFIG));
        self.inner.put_cf(cf, key, value.as_bytes())?;
        Ok(())
    }
}
