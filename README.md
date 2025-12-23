# Description
This is a Discord bot + Minecraft Mod + backend service combo to track who's on your Minecraft server. The Minecraft mod is written in Java (obviously) for Fabric 1.21.21 for now. I'll look into writing other types of mods if there's interest. I'll also see if I can automate porting the mod to different versions (since the functionality is basic and shouldn't change). 

# Documentation

## Setup

### Server Owners
Invite the bot to your Discord server. Then run `/oxeye connect` to generate a code. After that, go to the Minecraft server console (or be an Administrator) and run `/oxeye connect $CODE` where `$CODE` is the code that the Discord bot gives to you.

### Self host
The Dockerfile here should work. Configure the relevant environment variables as seen in `oxeye-backend/src/config.rs` if you care to change. The only relevant one is giving a Discord bot token. Obtaining one is trivial, you just want the `bot` and the `applications.commands`  The process will panic and tell you that it does not have a token if this is the case. After that, go to `config.json` in the config file that mod will generate and set the backend URL to the IP of the server + the port that you've chosen (`3000` by default). Also, make sure to setup a docker volume that your Sqlite database will live in. Docker Compose handles this nicely.

## Usage

### Discord Commands
The following commands are slash `/` commands. Prefix commands are kinda broken right now.
 - `/oxeye connect` generates a code that you can give to the Minecraft server.
 - `/oxeye list` lists all servers connected in the Discord server.
 - `/oxeye status $SERVER_NAME` shows the players who are connected on a given server.

### Minecraft Commands
 - `/oxeye connect $CODE` connects to a server.
 - `/oxeye disconnect` disconnects from the server.
 - `/oxeye status` shows the health of the backend and whether the Minecraft server is connected to a Discord server

