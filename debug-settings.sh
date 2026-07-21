#!/bin/bash

echo "=== Debug Settings Loading ==="
echo ""

echo "1. Project settings file:"
cat /Users/coachaek/workspace/createder/agent-harness/.harness/settings.json
echo ""

echo "2. Global settings file:"
cat ~/.config/momo-fetch/settings.json 2>/dev/null || echo "Not found"
echo ""

echo "3. Current directory:"
pwd
echo ""

echo "4. Environment variables:"
env | grep -E "(PROVIDER|MODEL|OPENROUTER)" || echo "No provider-related env vars"
echo ""

echo "5. Running with explicit provider to test:"
echo "   cargo run -- --provider openrouter --model google/gemma-4-26b-a4b-it:free"