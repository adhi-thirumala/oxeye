package org.adhiadhi.oxeyeMod;

import java.net.URISyntaxException;
import java.util.List;
import java.util.Queue;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicBoolean;

public class SyncManager {
  private static final AtomicBoolean syncing = new AtomicBoolean(false);
  private static final Queue<Runnable> pendingEvents = new ConcurrentLinkedQueue<>();
  private static volatile String lastSyncedBootId = null;
  private static volatile boolean needsSync = true;

  public static CompletableFuture<Void> sync(List<String> players) {
    syncing.set(true);
    OxeyeMod.LOGGER.info("Sync started, blocking join/leave events");

    try {
      return OxeyeHttp.sendSyncRequest(players)
          .thenRun(SyncManager::drainQueue)
          .exceptionally(e -> {
            OxeyeMod.LOGGER.error("Sync failed: " + e.getMessage());
            drainQueue();
            return null;
          });
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.error("Failed to send sync request: " + e.getMessage());
      drainQueue();
      return CompletableFuture.completedFuture(null);
    }
  }

  public static void onPlayerJoin(String playerName) {
    if (syncing.get()) {
      OxeyeMod.LOGGER.info("Sync in progress, queueing join for: " + playerName);
      pendingEvents.add(() -> sendJoin(playerName));
    } else {
      sendJoin(playerName);
    }
  }

  public static void onPlayerLeave(String playerName) {
    if (syncing.get()) {
      OxeyeMod.LOGGER.info("Sync in progress, queueing leave for: " + playerName);
      pendingEvents.add(() -> sendLeave(playerName));
    } else {
      sendLeave(playerName);
    }
  }

  private static void drainQueue() {
    syncing.set(false);
    String currentBootId = OxeyeHttp.getLastBootId();
    lastSyncedBootId = currentBootId;
    needsSync = false;
    OxeyeMod.LOGGER.info("Sync complete (Boot ID: " + currentBootId + "), draining " + pendingEvents.size() + " queued events");
    Runnable event;
    while ((event = pendingEvents.poll()) != null) {
      event.run();
    }
  }

  private static void sendJoin(String playerName) {
    try {
      OxeyeHttp.sendJoinRequest(playerName);
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.error("Failed to send join request: " + e.getMessage());
    }
  }

  private static void sendLeave(String playerName) {
    try {
      OxeyeHttp.sendLeaveRequest(playerName);
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.error("Failed to send leave request: " + e.getMessage());
    }
  }

  public static boolean isSyncing() {
    return syncing.get();
  }

  public static boolean needsSync() {
    return needsSync;
  }

  public static void markOutOfSync() {
    needsSync = true;
    OxeyeMod.LOGGER.info("Marked out of sync due to boot ID change");
  }

  public static String getLastSyncedBootId() {
    return lastSyncedBootId;
  }
}
