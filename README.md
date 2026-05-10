# Description
This is a Discord bot + Minecraft Mod + backend service combo to track who's on your Minecraft server. The Minecraft mod is written in Java (obviously) for Fabric 1.21.11 for now. I'll look into writing other types of mods if there's interest. I'll also see if I can automate porting the mod to different versions (since the functionality is basic and shouldn't change).

The backend is built with Rust (Axum for HTTP + Poise for Discord) and is available as an OCI image in the GitHub Packages registry for both AMD64 and ARM64 architectures.

# Features
- **Real-time player tracking** - See who's online on your Minecraft server from Discord
- **Visual player list** - Shows Minecraft skin heads for each online player in Discord embeds
- **Automatic skin detection** - Detects when players change their skins and updates automatically
- **Time online tracking** - Shows how long each player has been online
- **Multi-server support** - Link multiple Minecraft servers to different Discord servers
- **Self-healing** - Automatically recovers state after backend restarts
- **Optimized bandwidth** - Smart caching reduces network overhead by 67x
- **Fast in-memory tracking** - Uses hybrid storage (SQLite + RAM) for performance 

# Documentation

## Setup

### Quick Start with Docker
Pre-built OCI images are available in GitHub Packages for AMD64 and ARM64:
```bash
docker pull ghcr.io/adhi-thirumala/oxeye-backend:latest-amd64
# or
docker pull ghcr.io/adhi-thirumala/oxeye-backend:latest-arm64
```

Or use Docker Compose:
```yaml
version: '3.8'
services:
  oxeye:
    image: ghcr.io/adhi-thirumala/oxeye-backend:latest-amd64
    ports:
      - "3000:3000"
    environment:
      - DISCORD_TOKEN=your_token_here
      - PUBLIC_URL=http://your-server-ip:3000  # Important for image serving
      - DATABASE_PATH=/data/oxeye.db
    volumes:
      - oxeye-data:/data
volumes:
  oxeye-data:
```

### Server Owners
Invite the bot to your Discord server. Then run `/oxeye connect` to generate a code. After that, go to the Minecraft server console (or be an Administrator) and run `/oxeye connect $CODE` where `$CODE` is the code that the Discord bot gives to you.

### Self host
The Dockerfile here should work if you want to build from source. Pre-built OCI images are also available in GitHub Packages (see Quick Start above). 

Configure the relevant environment variables:
- `DISCORD_TOKEN` (required) - Your Discord bot token
- `DATABASE_PATH` (default: "oxeye.db") - Where to store the SQLite database
- `PORT` (default: 3000) - HTTP server port
- `PUBLIC_URL` (default: "http://localhost:3000") - Base URL for serving player head images (important!)
- `REQUEST_BODY_LIMIT` (default: 1MB) - Max request body size
- `REQUEST_TIMEOUT_SECS` (default: 30) - Request timeout
- Rate limiting variables (see `oxeye-backend/src/config.rs` for full list)

The Discord bot needs the `bot` and `applications.commands` scopes. Obtaining a token is trivial through the Discord Developer Portal. The process will panic and tell you that it does not have a token if this is the case. 

After that, go to `config.json` in the config file that the mod will generate and set the backend URL to the IP of the server + the port that you've chosen (`3000` by default). 

Make sure to setup a docker volume that your SQLite database will live in. Docker Compose handles this nicely (see example above).

## Usage

### Discord Commands
The following commands are slash `/` commands. Prefix commands are kinda broken right now.
 - `/oxeye connect <server_name>` generates a code that you can give to the Minecraft server. The code expires after 10 minutes.
 - `/oxeye list` lists all servers connected in the Discord server.
 - `/oxeye status <server_name>` shows the players who are connected on a given server, with their Minecraft skin heads and how long they've been online. Server names have autocomplete.

### Minecraft Commands
 - `/oxeye connect <code>` connects to a server using the code from Discord.
 - `/oxeye disconnect` disconnects from the server.
 - `/oxeye status` shows the health of the backend and whether the Minecraft server is connected to a Discord server.
 - `/oxeye sync` manually triggers a state sync with the backend (admin only). This is usually automatic after backend restarts.

## How It Works

### Architecture
The system uses a hybrid storage model for performance:
- **Persistent (SQLite)**: Server links, connection codes, Minecraft skins, rendered head images
- **In-Memory (RAM)**: Current online players for zero-contention, high-speed updates

### Player Skin Rendering
When a player joins, the mod sends their skin texture hash to the backend. If the backend hasn't seen that skin before, it requests the full skin data. The backend then:
1. Extracts the face layer (8x8) and helmet overlay (8x8) from the skin
2. Renders a 64x64 or 128x128 player head image
3. Caches the rendered head for future use
4. Generates composite status images with multiple player heads for Discord embeds

This approach reduces bandwidth by 67x compared to sending full skin data on every join. Each unique skin is only rendered once, even if multiple players use the same skin.

### Automatic Recovery
The backend generates a unique Boot ID on startup. The Minecraft mod tracks this ID and automatically triggers a `/sync` request when it detects a backend restart, ensuring the player list stays accurate even after restarts.

## System Requirements
- **Minecraft**: 1.21.11 or compatible
- **Fabric Loader**: 0.18.3+
- **Fabric API**: 0.140.0+
- **Backend**: Docker/Podman or any environment that can run the OCI image

## API Endpoints
The backend exposes several endpoints:

### For Minecraft Mod (requires API key)
- `POST /connect` - Redeem connection code
- `POST /join` - Report player join
- `POST /leave` - Report player leave  
- `POST /sync` - Sync full player list
- `POST /disconnect` - Disconnect server
- `GET /status` - Health check
- `POST /skin` - Upload skin data

### Public Endpoints
- `GET /health` - Health check
- `GET /heads/{texture_hash}.png` - Serve player head image (cached, immutable)
- `GET /status-image/{api_key_hash}.png` - Serve composite status image

