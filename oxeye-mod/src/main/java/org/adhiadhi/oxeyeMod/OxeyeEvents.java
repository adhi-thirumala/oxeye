package org.adhiadhi.oxeyeMod;


import net.fabricmc.fabric.api.networking.v1.PacketSender;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.network.ServerGamePacketListenerImpl;

import java.net.URISyntaxException;
import java.util.ArrayList;

public class OxeyeEvents {
  public static void onPlayerJoin(ServerGamePacketListenerImpl serverGamePacketListener, PacketSender packetSender, MinecraftServer minecraftServer) {
    String name = serverGamePacketListener.player.getName().getString();
    OxeyeMod.LOGGER.info("Player joined: " + name);
    try {
      OxeyeHttp.sendJoinRequest(name);
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.severe("Failed to send join request: " + e.getMessage());
    }
  }

  public static void onPlayerDisconnect(ServerGamePacketListenerImpl serverGamePacketListener, MinecraftServer minecraftServer) {
    String name = serverGamePacketListener.player.getName().getString();
    OxeyeMod.LOGGER.info("Player disconnected: " + name);
    try {
      OxeyeHttp.sendLeaveRequest(name);
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.severe("Failed to send leave request: " + e.getMessage());
    }
  }

  public static void onServerStarted(MinecraftServer minecraftServer) {
    OxeyeMod.LOGGER.info("Server started, sending sync request");
    try {
      OxeyeHttp.sendSyncRequest((ArrayList<String>) minecraftServer.getPlayerList().getPlayers().stream().map(player -> player.getName().getString()).toList());
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.severe("Failed to send sync request: " + e.getMessage());
    }
  }

  public static void onServerStopped(MinecraftServer minecraftServer) {
    OxeyeMod.LOGGER.info("Server stopped");
    try {
      OxeyeHttp.sendSyncRequest(new ArrayList<>());
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.severe("Failed to send sync request: " + e.getMessage());
    }
  }
}