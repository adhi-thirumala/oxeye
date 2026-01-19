package org.adhiadhi.oxeyeMod;


import com.mojang.authlib.GameProfile;
import net.fabricmc.fabric.api.networking.v1.PacketSender;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.network.ServerGamePacketListenerImpl;

import java.util.List;
import java.util.Optional;

public class OxeyeEvents {
  private static MinecraftServer currentServer;

  public static void onPlayerJoin(ServerGamePacketListenerImpl serverGamePacketListener, PacketSender packetSender, MinecraftServer minecraftServer) {
    String name = serverGamePacketListener.player.getName().getString();
    OxeyeMod.LOGGER.info("Player joined: " + name);
    
    // Extract skin info from the player's GameProfile
    GameProfile profile = serverGamePacketListener.player.getGameProfile();
    Optional<SkinUtil.SkinInfo> skinInfo = SkinUtil.extractSkinInfo(profile);
    
    // If backend restarted (boot ID changed), auto-sync first
    if (SyncManager.needsSync() && !SyncManager.isSyncing()) {
      List<String> currentPlayers = minecraftServer.getPlayerList().getPlayers().stream()
          .map(player -> player.getName().getString()).toList();
      SyncManager.sync(currentPlayers);
    }
    
    // Send join with skin info
    SyncManager.onPlayerJoin(name, skinInfo.orElse(null));
  }

  public static void onPlayerDisconnect(ServerGamePacketListenerImpl serverGamePacketListener, MinecraftServer minecraftServer) {
    String name = serverGamePacketListener.player.getName().getString();
    OxeyeMod.LOGGER.info("Player disconnected: " + name);
    SyncManager.onPlayerLeave(name);
  }

  public static void onServerStarted(MinecraftServer minecraftServer) {
    currentServer = minecraftServer;
    OxeyeMod.LOGGER.info("Server started, sending sync request");
    SyncManager.sync(minecraftServer.getPlayerList().getPlayers().stream()
        .map(player -> player.getName().getString()).toList());
  }

  public static void onServerStopped(MinecraftServer minecraftServer) {
    OxeyeMod.LOGGER.info("Server stopped");
    SyncManager.sync(List.of());
    currentServer = null;
  }

  public static MinecraftServer getCurrentServer() {
    return currentServer;
  }
}