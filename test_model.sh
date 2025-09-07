#!/bin/bash

echo "Testing OpenAI API directly with curl..."
echo ""

# Read the API key from .env
source .env

# Test with a known working model
curl https://api.openai.com/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "Say hello"}],
    "stream": true
  }' 2>/dev/null

echo ""
echo ""
echo "Now testing with the configured model..."

# Test with the model from .env
curl https://api.openai.com/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $OPENAI_API_KEY" \
  -d '{
    "model": "'$OPENAI_MODEL'",
    "messages": [{"role": "user", "content": "Say hello"}],
    "stream": true
  }' 2>/dev/null