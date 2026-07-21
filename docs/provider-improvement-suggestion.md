# ข้อเสนอแนะการปรับปรุง Provider System

## ปัญหาปัจจุบัน

1. ZAI provider ถูก hardcoded ใน `providers.rs`
2. การเพิ่ม OpenAI-compatible provider ใหม่ต้องแก้ไขโค้ด
3. การ config ผ่าน environment variables ไม่สมดุลกัน
4. ไม่มีวิธีง่ายๆ ในการจัดการ multiple OpenAI-compatible endpoints

## ข้อเสนอแนะการปรับปรุง

### 1. Generic OpenAI-Compatible Provider Factory

แทนที่จะมี provider-specific code สำหรับแต่ละ provider, สร้าง generic factory:

```rust
// src/providers/generic_factory.rs

use std::collections::HashMap;
use anyhow::Result;

pub struct GenericProviderConfig {
    pub name: String,
    pub api_key_env: String,
    pub base_url: String,
    pub default_model: String,
    pub model_param_name: String,  // "model" สำหรับส่วนใหญ่
    pub auth_header: String,       // "Bearer" สำหรับส่วนใหญ่
    pub supports_streaming: bool,
}

impl GenericProviderConfig {
    pub fn from_env(prefix: &str) -> Result<Self> {
        let name = prefix.to_lowercase();
        let api_key_env = format!("{}_API_KEY", prefix.to_uppercase());
        let base_url = std::env::var(format!("{}_URL", prefix.to_uppercase()))
            .or_else(|_| std::env::var(format!("{}_BASE_URL", prefix.to_uppercase())))?;
        let default_model = std::env::var(format!("{}_MODEL", prefix.to_uppercase()))
            .unwrap_or_else(|_| "default".to_string());
        
        Ok(Self {
            name,
            api_key_env,
            base_url,
            default_model,
            model_param_name: "model".to_string(),
            auth_header: "Bearer".to_string(),
            supports_streaming: true,
        })
    }
    
    pub fn create_openai_compatible_client(&self) -> Result<Arc<dyn Llm>> {
        let api_key = std::env::var(&self.api_key_env)?;
        let model = std::env::var(format!("{}_MODEL", self.name.to_uppercase()))
            .unwrap_or_else(|_| self.default_model.clone());
        
        let config = OpenAICompatibleConfig::new(&api_key, &model)
            .with_base_url(&self.base_url)
            .with_provider_name(&self.name);
        
        let client = OpenAICompatible::new(config)?;
        Ok(Arc::new(client))
    }
}
```

### 2. ปรับปรุง ProviderManager

```rust
// src/providers.rs

impl ProviderManager {
    pub fn from_env() -> anyhow::Result<Self> {
        // ลอง environment-specific providers ก่อน
        if let Ok(llm) = Self::try_specific_providers() {
            return Ok(llm);
        }
        
        // ลอง generic OpenAI-compatible providers
        if let Ok(llm) = Self::try_generic_providers() {
            return Ok(llm);
        }
        
        // Fallback ไป Ollama
        Self::ollama_provider()
    }
    
    fn try_specific_providers() -> anyhow::Result<Self> {
        use crate::config::secrets::SecretStore;
        
        // Anthropic (special case - ไม่ใช่ OpenAI-compatible)
        if let Ok(key) = SecretStore::get("anthropic") {
            let model = "claude-sonnet-4-20250514";
            let client = AnthropicClient::new(AnthropicConfig::new(&key, model))?;
            return Ok(Self::new("anthropic", model, Arc::new(client)));
        }
        
        // OpenAI (original API)
        if let Ok(key) = SecretStore::get("openai") {
            let model = "gpt-4o";
            let client = OpenAIClient::new(OpenAIConfig::new(&key, model))?;
            return Ok(Self::new("openai", model, Arc::new(client)));
        }
        
        Err(anyhow::anyhow!("No specific provider found"))
    }
    
    fn try_generic_providers() -> anyhow::Result<Self> {
        let generic_providers = vec![
            "ZAI",        // ZAI API
            "TOGETHER",   // Together AI
            "ANYSCALE",   // Anyscale
            "NOVU",       // Novu AI
            "VLLM",       // vLLM (local)
            "LM_STUDIO",  // LM Studio (local)
            "OPENROUTER", // OpenRouter
        ];
        
        for provider_prefix in generic_providers {
            if let Ok(config) = GenericProviderConfig::from_env(provider_prefix) {
                if let Ok(client) = config.create_openai_compatible_client() {
                    return Ok(Self::new(
                        &config.name,
                        &config.default_model,
                        client
                    ));
                }
            }
        }
        
        Err(anyhow::anyhow!("No generic provider found"))
    }
    
    fn ollama_provider() -> anyhow::Result<Self> {
        let model = "llama3.2";
        let client = OllamaModel::new(OllamaConfig::new(&model))?;
        Ok(Self::new("ollama", model, Arc::new(client)))
    }
}
```

### 3. Config File Support

สร้าง `~/.config/momo-fetch/providers.toml`:

```toml
# Generic OpenAI-compatible providers
[providers.zai]
name = "ZAI"
base_url = "https://api.z.ai/api/coding/paas/v4"
api_key_env = "ZAI_API_KEY"
default_model = "GLM-4.7"

[providers.together]
name = "Together AI"
base_url = "https://api.together.xyz/v1"
api_key_env = "TOGETHER_API_KEY"
default_model = "mistralai/Mixtral-8x7B-Instruct-v0.1"

[providers.vllm]
name = "vLLM"
base_url = "http://localhost:8000/v1"
api_key_env = "VLLM_API_KEY"  # Optional
default_model = "meta-llama/Llama-3-8b"

[providers.lm_studio]
name = "LM Studio"
base_url = "http://localhost:1234/v1"
api_key_env = ""  # Empty = no auth required
default_model = "local-model"
```

### 4. ปรับปรุง Secret Store

```rust
// src/config/secrets.rs

impl SecretStore {
    pub fn get_optional(provider: &str) -> Option<String> {
        let env_var = env_var_for_provider(provider);
        
        // Check environment first
        if let Ok(key) = std::env::var(&env_var) {
            return Some(key);
        }
        
        // Try OS keychain
        let entry = keyring_core::Entry::new(SERVICE_NAME, provider).ok()?;
        entry.get_password().ok()
    }
    
    pub fn is_available(provider: &str) -> bool {
        Self::get_optional(provider).is_some()
    }
}
```

### 5. Provider Discovery

```rust
// src/providers/discovery.rs

use std::collections::HashMap;

pub struct ProviderDiscovery {
    detected_providers: HashMap<String, ProviderInfo>,
}

impl ProviderDiscovery {
    pub fn discover() -> Self {
        let mut discovered = HashMap::new();
        
        // Check for common OpenAI-compatible providers
        let common_providers = vec![
            ("ZAI", "https://api.z.ai/api/coding/paas/v4"),
            ("TOGETHER", "https://api.together.xyz/v1"),
            ("ANYSCALE", "https://api.endpoints.anyscale.com/v1"),
            ("VLLM", "http://localhost:8000/v1"),
            ("LM_STUDIO", "http://localhost:1234/v1"),
        ];
        
        for (prefix, default_url) in common_providers {
            let env_key = format!("{}_API_KEY", prefix);
            let env_url = format!("{}_URL", prefix);
            let env_model = format!("{}_MODEL", prefix);
            
            if std::env::var(&env_key).is_ok() || std::env::var(&env_url).is_ok() {
                discovered.insert(
                    prefix.to_lowercase(),
                    ProviderInfo {
                        name: prefix.to_lowercase(),
                        base_url: std::env::var(&env_url)
                            .unwrap_or_else(|_| default_url.to_string()),
                        api_key_env: env_key,
                        default_model: std::env::var(&env_model)
                            .unwrap_or_else(|_| "default".to_string()),
                    }
                );
            }
        }
        
        Self { detected_providers: discovered }
    }
    
    pub fn list_available(&self) -> Vec<&ProviderInfo> {
        self.detected_providers.values().collect()
    }
}
```

## ประโยชน์ของการปรับปรุง

1. **ยืดหยุ่น**: รองรับ OpenAI-compatible provider ใหม่ๆ โดยไม่ต้องแก้ไขโค้ด
2. **สมดุล**: API key, base URL, model ถูก config แบบเดียวกันทุก provider
3. **Auto-discovery**: ตรวจพบ provider ที่ config ไว้โดยอัตโนมัติ
4. **Easy testing**: ง่ายในการสลับระหว่าง provider ต่างๆ
5. **Better error messages**: รู้ว่า provider ไหนไม่พร้อมใช้งานและทำไม

## การย้ายไปใช้ระบบใหม่

### Phase 1: เพิ่ม Generic Factory
1. สร้าง `generic_factory.rs`
2. เพิ่ม `GenericProviderConfig`
3. ทดสอบกับ ZAI provider

### Phase 2: Auto-discovery
1. สร้าง `discovery.rs`
2. เพิ่ม provider detection
3. แสดง available providers ใน CLI

### Phase 3: Config File
1. สร้าง TOML config support
2. Migration guide จาก .env
3. Validation และ testing

## Example Usage

หลังจากปรับปรุง:

```bash
# ตรวจสอบ providers ที่พร้อมใช้งาน
/provider list

# สลับไป provider ที่ auto-detect ได้
/provider zai
/model GLM-4.7

# หรือใช้ provider ที่เพิ่มใหม่
/provider together
/model mistralai/Mixtral-8x7B-Instruct-v0.1

# Local provider
/provider vllm
/model meta-llama/Llama-3-8b
```

## Migration Path

จากปัจจุบัน:

```env
# Old way (ยังใช้งานได้)
ZAI_API_KEY=key
ZAI_URL=url
ZAI_MODEL=model
```

ไปยัง:

```env
# New generic way (รองรับทุก provider)
PROVIDER_NAME=together
PROVIDER_URL=https://api.together.xyz/v1
PROVIDER_API_KEY=key
PROVIDER_MODEL=model
```

หรือใช้ config file:

```toml
[providers.together]
base_url = "https://api.together.xyz/v1"
api_key_env = "TOGETHER_API_KEY"
default_model = "mistralai/Mixtral-8x7B-Instruct-v0.1"
```