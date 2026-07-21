# Settings.json Architecture Issue & Solutions

## 🐛 ปัญหา

`settings.json` ไม่สามารถตั้งค่า provider/model ได้จริง!

### เหตุผลทางเทคนิค:

```rust
// src/config/mod.rs
pub struct HarnessConfig {
    pub provider: ProviderSettings,  // สร้างจาก settings.json
    pub memory: MemorySettings,      // สร้างจาก settings.json
    // ...
}

// แต่ ProviderManager::from_env() ไม่ได้รับพารามิเตอร์!
// มันอ่านเฉพาะ environment variables + keychain
impl ProviderManager {
    pub fn from_env() -> anyhow::Result<Self> {
        // อ่านเฉพาะ ANTHROPIC_API_KEY, OPENAI_API_KEY, etc.
        // ไม่ได้อ่านจาก HarnessConfig.provider
    }
}
```

## ✅ วิธีแก้ปัญหา (3 ทางเลือก)

### วิธีที่ 1: Environment Variables (แนะนำสุด)

```bash
# .env
OPENROUTER_API_KEY=sk-or-...
OPENROUTER_MODEL=google/gemma-4-26b-a4b-it:free
```

**ข้อดี:**
- ทำงานจริงทันที
- ไม่ต้องแก้โค้ด
- ProviderManager อ่านได้จริง

**ข้อเสีย:**
- ต้องตั้งค่าใน 2 ไฟล์ (.env + settings.json)

### วิธีที่ 2: CLI Args

```bash
cargo run -- --provider openrouter --model google/gemma-4-26b-a4b-it:free
```

**ข้อดี:**
- ทำงานได้จริง
- Override ได้ทันที

**ข้อเสีย:**
- ต้องพิมพ์ทุกครั้ง
- ไม่ permanent

### วิธีที่ 3: แก้ไขโค้ด (ต้องเปลี่ยน architecture)

**ต้องแก้ไขหลายจุด:**

1. **แก้ `ProviderManager::from_env()`**
```rust
impl ProviderManager {
    pub fn from_env_with_config(config: &ProviderSettings) -> anyhow::Result<Self> {
        // ใช้ config.default_provider และ config.default_model
    }
}
```

2. **แก้ `HarnessConfig::build()`**
```rust
pub fn build(config: HarnessConfig) -> anyhow::Result<Self> {
    let provider_manager = ProviderManager::from_env_with_config(&config.provider)?;
    // ...
}
```

**ข้อดี:**
- settings.json ทำงานได้จริง
- เป็นระบบที่ถูกต้อง

**ข้อเสีย:**
- ต้องแก้โค้ดหลายจุด
- อาจทำลาย backward compatibility
- ต้องทดสอบใหม่ทั้งหมด

## 🎯 สิ่งที่ settings.json ทำได้จริง:

settings.json ใช้สำหรับ:

✅ **`permission_mode`** - strict, auto, yolo  
✅ **`memory` settings** - auto_search, auto_write, sidecar options  
✅ **`context_window` overrides** - model context sizes  

❌ **`default_provider`** - ProviderManager ไม่อ่าน  
❌ **`default_model`** - ProviderManager ไม่อ่าน  

## 📋 สรุปสิ่งที่ต้องทำตอนนี้:

### วิธีที่ 1 (Environment Variables - แนะนำ):

```bash
# .env
OPENROUTER_API_KEY=sk-or-...
OPENROUTER_MODEL=google/gemma-4-26b-a4b-it:free

# .harness/settings.json  
{
  "permission_mode": "auto",
  "memory": {
    "sidecar_model": "google/gemma-4-26b-a4b-it:free",
    "sidecar_provider": "openrouter"
  }
}
```

### วิธีที่ 2 (CLI Args):

```bash
cargo run -- --provider openrouter --model google/gemma-4-26b-a4b-it:free
```

## 🔧 สำหรับอนาคต:

ถ้าต้องการให้ settings.json ทำงานได้จริงกับ provider/model:

1. แก้ `ProviderManager::from_env()` ให้รับ `ProviderSettings`
2. แก้ `HarnessConfig::build()` ให้ส่ง `ProviderSettings` ไป
3. เพิ่ม fallback logic: CLI > ENV > settings.json > defaults
4. ทดสอบทั้งหมดใหม่

**แต่ตอนนี้:** ใช้ environment variables แทน!