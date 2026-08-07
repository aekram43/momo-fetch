# LLM Provider Configuration Guide

## Overview
ระบบรองรับ LLM provider หลายประเภท ทั้ง provider หลักและ OpenAI-compatible providers ทั่วไป

## ปัญหาปัจจุบันและวิธีแก้ไข

### 401 Unauthorized Error
```
zai API error (HTTP 401 Unauthorized): token expired or incorrect
```

**วิธีแก้ไข:**
1. ตรวจสอบ API key ว่ายังใช้งานได้หรือไม่
2. อัปเดต API key ใน `.env` หรือ keychain:
   ```bash
   # อัปเดตใน .env
   ZAI_API_KEY=new_api_key_here
   
   # หรือใช้คำสั่ง /key ใน REPL
   /key set zai new_api_key_here
   ```

## การรองรับ OpenAI-Compatible Provider ทุกประเภท

ระบบรองรับ OpenAI-compatible providers ผ่าน 3 วิธี:

### วิธีที่ 1: Generic Custom Provider (แนะนำ)

ใช้ environment variables เหล่านี้สำหรับ OpenAI-compatible API ใดก็ได้:

```bash
# ใน .env
LLM_URL=https://api.provider.com/v1
LLM_API_KEY=your_api_key
LLM_MODEL=model_name
```

**ตัวอย่าง Provider ยอดนิยม:**

#### vLLM (Local)
```env
LLM_URL=http://localhost:8000/v1
LLM_API_KEY=optional_token
LLM_MODEL=meta-llama/Llama-3-8b
```

#### LM Studio (Local)
```env
LLM_URL=http://localhost:1234/v1
LLM_API_KEY=lm-studio
LLM_MODEL=loaded_model_name
```

#### Together AI
```env
LLM_URL=https://api.together.xyz/v1
LLM_API_KEY=your_together_api_key
LLM_MODEL=mistralai/Mixtral-8x7B-Instruct-v0.1
```

#### Anyscale
```env
LLM_URL=https://api.endpoints.anyscale.com/v1
LLM_API_KEY=your_anyscale_api_key
LLM_MODEL=meta-llama/Llama-2-70b-chat-hf
```

#### Novu AI
```env
LLM_URL=https://api.nuvalabs.io/v1
LLM_API_KEY=your_novu_api_key
LLM_MODEL=novu/model-name
```

### วิธีที่ 2: ใช้ Provider ที่มีอยู่แล้ว

#### ZAI (Current Setup)
```env
ZAI_API_KEY=your_zai_key
ZAI_URL=https://api.z.ai/api/coding/paas/v4
ZAI_MODEL=GLM-4.7
```

#### OpenRouter (Multi-provider)
```env
OPENROUTER_API_KEY=sk-or-...
# Model format: provider/model
OPENROUTER_MODEL=anthropic/claude-sonnet-4
```

### วิธีที่ 3: Ollama (Local, No API Key)
```env
# ไม่ต้องใส่ API key
OLLAMA_MODEL=llama3.2
# หรือระบุ URL ถ้าไม่ใช่ localhost
OLLAMA_BASE_URL=http://192.168.1.100:11434
```

## การตรวจสอบและแก้ไขปัญหา

### ตรวจสอบ Provider ที่พร้อมใช้งาน
```bash
# ใน REPL
/provider list
```

### สลับ Provider
```bash
# สลับไป provider อื่น
/provider openai
/model gpt-4o

# หรือใช้ custom provider
/provider custom
/model your-model-name

# สลับทั้ง provider และ model ในคำสั่งเดียว
/provider openai --model gpt-4o-mini
```

### ดูข้อมูล API key ที่เก็บไว้
```bash
/key list
```

### ตั้งค่า API key ใหม่
```bash
# เก็บใน keychain (ปลอดภัยกว่า)
/key set openai sk-...

# หรือแก้ไข .env โดยตรง
nano .env
```

## Provider Comparison Table

| Provider | Auth Method | Base URL | Model Format | Local/Cloud |
|----------|-------------|----------|--------------|-------------|
| OpenAI | API Key | https://api.openai.com/v1 | gpt-4o, gpt-4o-mini | Cloud |
| Anthropic | API Key | https://api.anthropic.com | claude-sonnet-4-... | Cloud |
| ZAI | API Key | Custom URL | GLM-4.7, GLM-5 | Cloud |
| vLLM | Optional | http://localhost:8000/v1 | any HuggingFace model | Local |
| Ollama | None | http://localhost:11434 | llama3.2, mistral | Local |
| LM Studio | None | http://localhost:1234/v1 | loaded model | Local |
| OpenRouter | API Key | https://openrouter.ai/api/v1 | provider/model | Cloud |
| Together AI | API Key | https://api.together.xyz/v1 | org/model | Cloud |
| Generic Custom | API Key | Custom URL | any | Both |

## Best Practices

1. **Security**
   - ใช้ keychain แทน `.env` สำหรับ production
   - ไม่ commit `.env` ไปยัง git
   - Rotate API keys สม่ำเสมอ

2. **Performance**
   - Local providers (Ollama, vLLM) เร็วกว่าสำหรับ small models
   - Cloud providers ดีกว่าสำหรับ large models และ high availability

3. **Cost Management**
   - ใช้ local providers สำหรับ development
   - ใช้ cloud providers สำหรับ production เมื่อต้องการ high quality

4. **Fallback Strategy**
   - ตั้งค่า primary provider
   - มี backup provider พร้อม
   - ใช้ local provider เป็น fallback สุดท้าย

## Troubleshooting

### "No secret set for provider"
- แก้ไข: ตั้งค่า API key ด้วย `/key set <provider> <key>`

### "Unknown provider"
- แก้ไข: ใช้ "custom" หรือเพิ่ม provider ใน config

### "Connection refused"
- แก้ไข: ตรวจสอบ base URL และ network connectivity

### "401 Unauthorized"
- แก้ไข: ตรวจสอบ API key ว่าถูกต้องและไม่หมดอายุ

### "Model not found"
- แก้ไข: ตรวจสอบ model name ว่าถูกต้องกับ provider นั้นๆ

## Advanced Configuration

สำหรับ provider ที่ต้องการการ config พิเศษ (เช่น Azure OpenAI, พร็อพเพอร์ตี้เพิ่มเติม):

แก้ไข `src/providers.rs` เพื่อเพิ่ม provider config เฉพาะ:

```rust
"azure" => {
    let key = SecretStore::get("azure")?;
    let endpoint = std::env::var("AZURE_OPENAI_ENDPOINT")?;
    let deployment = std::env::var("AZURE_OPENAI_DEPLOYMENT")?;
    let api_version = std::env::var("AZURE_OPENAI_API_VERSION")
        .unwrap_or_else(|_| "2024-02-01".to_string());
    
    let config = OpenAICompatibleConfig::new(&key, &deployment)
        .with_base_url(&endpoint)
        .with_api_version(&api_version)
        .with_provider_name("azure");
    
    let client = OpenAICompatible::new(config)?;
    Ok(Arc::new(client))
}
```