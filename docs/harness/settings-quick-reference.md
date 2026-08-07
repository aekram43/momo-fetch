# Settings.json Quick Reference

## สรุปตัวเลือกทั้งหมดที่ตั้งค่าได้

### 🔧 Basic Settings
```json
{
  "permission_mode": "auto",           // strict | auto | yolo
  "default_provider": "anthropic",     // anthropic | openai | deepseek | groq | openrouter | ollama | zai | custom
  "default_model": "claude-sonnet-4-20250514"
}
```

### 🧠 Memory Settings
```json
{
  "memory": {
    "auto_search": true,              // Auto-search memory before each turn
    "auto_write": true,               // Auto-save conversations to memory
    "search_mode": "grep_llm",        // grep_llm | tag_filter
    "max_results_per_turn": 5,        // Max memory results to inject (3-10 recommended)
    "extract_threshold": 10,          // Turns before auto-extract (0 to disable)
    "sidecar_model": null,            // Option B: Use cheaper model for memory
    "sidecar_provider": null,         // Provider for sidecar model
    "consolidate_threshold": 30       // MemCells before auto-consolidate (0 to disable)
  }
}
```

### 🪟 Context Window Overrides
```json
{
  "context_window": {
    "model_name": 200000              // Override auto-detected context size
  }
}
```

## 🚨 แก้ปัญหา ZAI 401 Unauthorized

### วิธีที่ 1: อัปเดต API key
```bash
# แก้ไข .env
ZAI_API_KEY=your_new_valid_key
```

### วิธีที่ 2: เปลี่ยน provider (แนะนำ)
```json
{
  "memory": {
    "sidecar_model": "deepseek-chat",
    "sidecar_provider": "deepseek"
  }
}
```

### วิธีที่ 3: ปิด sidecar (ใช้ Option A - TF-IDF)
```json
{
  "memory": {
    "sidecar_model": null,
    "sidecar_provider": null
  }
}
```

## 📋 ตัวอย่างการตั้งค่ายอดนิยม

### 💸 ราคาถูก + คุณภาพดี (แนะนำสำหรับ development)
```json
{
  "permission_mode": "auto",
  "default_provider": "deepseek",
  "default_model": "deepseek-chat",
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_provider": "groq",
    "sidecar_model": "llama-3.3-70b-versatile"
  }
}
```

### 💎 คุณภาพสูงสุด (แนะนำสำหรับ production)
```json
{
  "permission_mode": "auto",
  "default_provider": "anthropic",
  "default_model": "claude-sonnet-4-20250514",
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_provider": "deepseek",
    "sidecar_model": "deepseek-chat"
  }
}
```

### 🏠 Local 100% (ไม่ต้องใช้ API key)
```json
{
  "permission_mode": "yolo",
  "default_provider": "ollama",
  "default_model": "llama3.2",
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_provider": "ollama",
    "sidecar_model": "llama3.2"
  }
}
```

### 🌐 Multi-Provider (OpenRouter + ราคาถูกสำหรับ sidecar)
```json
{
  "permission_mode": "auto",
  "default_provider": "openrouter",
  "default_model": "anthropic/claude-sonnet-4",
  "memory": {
    "auto_search": true,
    "auto_write": true,
    "sidecar_provider": "groq",
    "sidecar_model": "llama-3.3-70b-versatile"
  }
}
```

## 🔧 Generic OpenAI-Compatible Provider

### ตั้งค่าใน .env
```bash
LLM_URL=https://api.provider.com/v1
LLM_API_KEY=your_api_key
LLM_MODEL=model_name
```

### ตั้งค่าใน settings.json
```json
{
  "default_provider": "custom",
  "default_model": "model_name"
}
```

## 📊 Provider ที่รองรับทั้งหมด

| Provider | Default Model | API Key Required | Best For |
|----------|---------------|------------------|----------|
| anthropic | claude-sonnet-4-20250514 | Yes | Premium quality |
| openai | gpt-4o | Yes | General purpose |
| deepseek | deepseek-chat | Yes | Cheap + good quality |
| groq | llama-3.3-70b-versatile | Yes | Fast (free tier) |
| openrouter | anthropic/claude-sonnet-4 | Yes | Multi-provider access |
| ollama | llama3.2 | No | Local models |
| zai | GLM-4.7 | Yes | Chinese models (check API key!) |
| custom | (any) | Varies | Any OpenAI-compatible API |

## 🎯 Permission Modes

- **strict**: ถามก่อน run ทุกคำสั่ง (default - ปลอดภัยสุด)
- **auto**: อนุญาตคำสั่ง read-only อัตโนมัติ, ถามก่อน write
- **yolo**: อนุญาตทุกคำสั่งอัตโนมัติ (ใช้ด้วยความระมัดระวัง!)

## 🚨 Troubleshooting

### 401 Unauthorized Error
- **สาเหตุ**: API key หมดอายุหรือไม่ถูกต้อง
- **วิธีแก้**: 
  1. อัปเดต API key ใน `.env`
  2. หรือเปลี่ยน provider

### "No secret set for provider"
- **สาเหตุ**: ไม่ได้ตั้งค่า API key
- **วิธีแก้**: 
  ```bash
  /key set provider_name your_api_key
  ```

### "Unknown provider"
- **สาเหตุ**: ระบุ provider ที่ไม่รองรับ
- **วิธีแก้**: ใช้ provider ที่รองรับหรือ "custom"

### Memory sidecar ไม่ทำงาน
- **ตรวจสอบ**: sidecar_model และ sidecar_provider ถูกต้อง
- **วิธีแก้**: ตั้งเป็น `null` เพื่อใช้ Option A (TF-IDF)

## 📝 Files ที่สร้างให้

1. **settings.json.sample** - เวอร์ชันเต็มพร้อมคำอธิบายทุกตัวเลือก
2. **settings.json.example** - เวอร์ชันกระชับ ใช้งานได้เลย
3. **settings.json.fix-zai-401** - คอนฟิกที่แนะนำสำหรับแก้ปัญหา ZAI 401

## 🎯 ถัดไป

1. เลือก config ที่เหมาะกับความต้องการ
2. Copy ไป `.harness/settings.json`
3. ตั้งค่า API keys ใน `.env` หรือ keychain
4. เริ่มใช้งาน!

สำหรับรายละเอียดเพิ่มเติม ดู:
- `settings.json.sample` - เอกสารครบถ้วน
- `docs/llm-provider-guide.md` - คู่มือ provider ทั้งหมด