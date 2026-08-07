# OpenRouter Model Recommendations

## 🆓 Free Models (Best for development)

### Google Gemma 4 26B (Current)
```json
{
  "default_model": "google/gemma-4-26b-a4b-it:free",
  "memory": {
    "sidecar_model": "google/gemma-4-26b-a4b-it:free"
  }
}
```
**Pros:** ฟรี, 26B params, คุณภาพดี  
**Cons:** มี rate limits, ความเร็วปานกลาง

### Mistral 7B
```json
{
  "default_model": "mistralai/mistral-7b-instruct:free",
  "memory": {
    "sidecar_model": "mistralai/mistral-7b-instruct:free"
  }
}
```
**Pros:** ฟรี, 7B params, เร็วกว่า  
**Cons:** ความฉลาดน้อยกว่า Gemma

## 💰 Budget Models (Best value)

### Llama 3 8B
```json
{
  "default_model": "meta-llama/llama-3-8b-instruct",
  "memory": {
    "sidecar_model": "meta-llama/llama-3-8b-instruct"
  }
}
```
**Cost:** $0.03/1M tokens  
**Pros:** ราคาถูกมาก, คุณภาพดี  
**Cons:** ไม่ฟรีแต่ราคาถูกมาก

### Mistral 7B (Paid)
```json
{
  "default_model": "mistralai/mistral-7b-instruct",
  "memory": {
    "sidecar_model": "mistralai/mistral-7b-instruct"
  }
}
```
**Cost:** $0.07/1M tokens  
**Pros:** เร็ว, คุณภาพดีสำหรับ task ทั่วไป

## 💎 Premium Models (Best quality)

### Claude Sonnet 4
```json
{
  "default_model": "anthropic/claude-sonnet-4",
  "memory": {
    "sidecar_model": "google/gemma-4-26b-a4b-it:free"
  }
}
```
**Cost:** $3/1M tokens  
**Pros:** คุณภาพสูงสุด, 200K context  
**Cons:** แพงมากสำหรับ sidecar

### GPT-4o Mini
```json
{
  "default_model": "openai/gpt-4o-mini",
  "memory": {
    "sidecar_model": "meta-llama/llama-3-8b-instruct"
  }
}
```
**Cost:** $0.15/1M tokens  
**Pros:** คุณภาพดี, ราคาปานกลาง  
**Cons:** 128K context (น้อยกว่า Claude)

## 🎯 Recommended Combinations

### 💸 Budget-friendly
```json
{
  "default_provider": "openrouter",
  "default_model": "meta-llama/llama-3-8b-instruct",
  "memory": {
    "sidecar_model": "google/gemma-4-26b-a4b-it:free",
    "sidecar_provider": "openrouter"
  }
}
```

### 💎 Premium quality
```json
{
  "default_provider": "openrouter",
  "default_model": "anthropic/claude-sonnet-4",
  "memory": {
    "sidecar_model": "meta-llama/llama-3-8b-instruct",
    "sidecar_provider": "openrouter"
  }
}
```

### 🆓 100% Free
```json
{
  "default_provider": "openrouter",
  "default_model": "google/gemma-4-26b-a4b-it:free",
  "memory": {
    "sidecar_model": "google/gemma-4-26b-a4b-it:free",
    "sidecar_provider": "openrouter"
  }
}
```

## 🔧 Troubleshooting

### Rate Limit Errors
**Problem:** Too many requests to free model  
**Solution:** 
- รอสักครู่แล้วลองใหม่
- หรือเปลี่ยนไปใช้ paid model

### Model Not Available
**Problem:** "Model not found" error  
**Solution:** 
- ตรวจสอบ model name ที่ https://openrouter.ai/models
- ใช้ format: `provider/model-name`

### Poor Performance
**Problem:** Model ตอบผิดหรือช้า  
**Solution:** 
- Free models: เปลี่ยนเป็น `mistral-7b-instruct:free`
- Paid models: เปลี่ยนเป็น `llama-3-8b-instruct`

## 📊 Cost Comparison

| Model | Cost/1M tokens | Quality | Speed | Recommended |
|-------|---------------|---------|-------|-------------|
| gemma-4-26b:free | $0 | ⭐⭐⭐ | ⭐⭐⭐ | ✅ Development |
| mistral-7b:free | $0 | ⭐⭐ | ⭐⭐⭐⭐ | ✅ Simple tasks |
| llama-3-8b | $0.03 | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ✅ Production |
| gpt-4o-mini | $0.15 | ⭐⭐⭐⭐ | ⭐⭐⭐⭐ | ✅ Balanced |
| claude-sonnet-4 | $3.00 | ⭐⭐⭐⭐⭐ | ⭐⭐⭐ | ✅ Premium |

## 🎓 Learning Tips

1. **Start with free models** - ทดลองกับ `gemma-4-26b:free`
2. **Upgrade for production** - ใช้ `llama-3-8b` เมื่อต้องการความเสถียร
3. **Premium for complex tasks** - ใช้ `claude-sonnet-4` เมื่อต้องการคุณภาพสูงสุด
4. **Mix and match** - main ใช้แพง, sidecar ใช้ถูก

## 📖 References

- OpenRouter Models: https://openrouter.ai/models
- Pricing: https://openrouter.ai/docs#models
- Rate Limits: https://openrouter.ai/docs#rate-limits