# 🤖 AI iMessage Bot

A powerful AI-powered iMessage bot built in Rust that integrates with BlueBubbles to provide automated responses in group chats and direct messages.

## ✨ Features

- **🧠 Multi-AI Support**: OpenAI GPT-4o and local Ollama models
- **🎭 Dynamic Characters**: AI-generated character personalities per chat
- **🗣️ Natural Language Triggers**: Respond to "myai hello" instead of just "@myai"
- **🎨 Image Generation**: DALL-E integration for creating and sending images
- **⚡ Multi-Chat Support**: Independent agents for each conversation
- **🔄 Async Message Queue**: Non-blocking message processing
- **💾 Persistent Storage**: SQLite database for chat configs and history

## 🚀 Quick Start

### Prerequisites

- macOS with iMessage configured
- Rust 1.70+
- BlueBubbles Server

### Installation

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone repository
git clone <your-repo-url>
cd ai-imessage

# Setup environment
cp .env.example .env
```

### Configuration

Edit `.env` with your settings:

```env
# BlueBubbles Configuration
BLUEBUBBLES_API=http://localhost:12345
BLUEBUBBLES_PASSWORD=your_password_here

# AI Configuration (choose one or both)
OPENAI_API_KEY=sk-your-openai-key-here
OLLAMA_API=http://localhost:11434
OLLAMA_MODEL=llama3.2

# Bot Settings
BOT_TRIGGER=@myai
DATABASE_URL=sqlite:./bot.db
RUST_LOG=info
```

### BlueBubbles Setup

1. **Download**: Get BlueBubbles Server from [bluebubbles.app](https://bluebubbles.app)
2. **Install**: Follow the setup wizard to connect your iMessage
3. **Configure**:
   - Set API password in BlueBubbles settings
   - Enable HTTP API on port 12345
   - Prevent background throttling:
     ```bash
     defaults write com.bluebubbles.server NSAppSleepDisabled -bool YES
     ```

### Run the Bot

```bash
# Build and run
cargo run

# Or build release version
cargo build --release
./target/release/ai-imessage-bot
```

## 💬 Usage

### Basic Commands

| Command | Description | Example |
|---------|-------------|---------|
| Natural trigger | Chat naturally with the bot | `myai hello there` |
| `@character <desc>` | Change bot personality | `@character friendly pirate` |
| `@name <name>` | Change trigger word | `@name assistant` |
| `@unhinge <true/false>` | Switch AI models | `@unhinge true` |

### Examples

```
You: myai hello
Bot: Hey there! How can I help you today?

You: @character grumpy cat
Bot: ✅ Character updated! I'm now: grumpy cat

You: myai how are you?  
Bot: *grumbles* What do you want now? Can't you see I'm busy being grumpy?

You: @name bot
Bot: ✅ Trigger name changed from 'myai' to 'bot'. You can now say 'bot, hello!' instead of using @

You: bot generate me a sunset image
Bot: ✅ Generated and sent a picture!
```

## 🏗️ Architecture

- **BotOrchestrator**: Main controller managing chat agents and message polling
- **ChatAgent**: Individual agents handling message processing per chat
- **MessageQueue**: Async processing system preventing blocking
- **Database**: SQLite storage for configurations and chat history
- **Commands**: Parser for bot commands (@character, @unhinge, @name)
- **AI Clients**: Unified interface for OpenAI and Ollama

## 🔧 Development

### Building

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test

# Check code
cargo check
```

### Database

The bot uses SQLite with these tables:
- `chat_configs`: Per-chat settings (character, triggers, model preference)
- `chat_contexts`: Message history for conversation context
- `processed_messages`: Tracking to prevent duplicate processing
- `message_queue`: Async processing queue

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `BLUEBUBBLES_API` | BlueBubbles server URL | `http://localhost:12345` |
| `BLUEBUBBLES_PASSWORD` | BlueBubbles API password | Required |
| `OPENAI_API_KEY` | OpenAI API key | Optional |
| `OLLAMA_API` | Ollama server URL | `http://localhost:11434` |
| `OLLAMA_MODEL` | Ollama model name | `llama3.2` |
| `BOT_TRIGGER` | Default trigger word | `@myai` |
| `DATABASE_URL` | SQLite database path | `sqlite:./bot.db` |
| `RUST_LOG` | Logging level | `info` |

## 🐛 Troubleshooting

### BlueBubbles Issues

**Slow responses when BlueBubbles is in background:**
```bash
# Prevent App Nap
defaults write com.bluebubbles.server NSAppSleepDisabled -bool YES

# Keep alive with curl ping
while true; do
    curl -s http://localhost:12345/api/v1/server/info?password=your_password > /dev/null
    sleep 30
done &
```

**Connection errors:**
- Check BlueBubbles is running and API is enabled
- Verify password matches in `.env` and BlueBubbles settings
- Ensure port 12345 is not blocked

### Bot Issues

**Not responding to messages:**
- Check `RUST_LOG=debug` for detailed logs
- Verify trigger words are correct
- Ensure messages are newer than bot startup time

**Database errors:**
- Delete `bot.db` to reset (loses chat history)
- Check database permissions

## 🤝 Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests if applicable
5. Submit a pull request

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## 🙏 Acknowledgments

- [BlueBubbles](https://bluebubbles.app) - iMessage bridge
- [OpenAI](https://openai.com) - GPT-4o and DALL-E APIs
- [Ollama](https://ollama.ai) - Local language models