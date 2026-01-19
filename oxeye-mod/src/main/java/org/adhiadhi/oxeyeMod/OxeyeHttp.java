package org.adhiadhi.oxeyeMod;

import com.google.gson.Gson;

import java.net.URI;
import java.net.URISyntaxException;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CompletableFuture;

public class OxeyeHttp {
  private static final HttpClient client = HttpClient.newHttpClient();
  private static final Gson GSON = new Gson();
  private static volatile String lastBootId = null;

  /**
   * Send a join request with optional skin information.
   * If the backend returns 202, it means we need to upload the skin data.
   *
   * @param playerName The player's name
   * @param skinInfo   Optional skin information (null if not available)
   */
  public static void sendJoinRequest(String playerName, SkinUtil.SkinInfo skinInfo) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending join request for player: " + playerName +
        (skinInfo != null ? " (with texture hash)" : " (no skin info)"));

    // Build the request body
    StringBuilder json = new StringBuilder();
    json.append("{\"player\":\"").append(playerName).append("\"");
    if (skinInfo != null) {
      json.append(",\"texture_hash\":\"").append(skinInfo.textureHash).append("\"");
    }
    json.append("}");

    // Send the join request
    sendJoinRequestWithSkinHandling(playerName, json.toString(), skinInfo);
  }

  /**
   * Send join request and handle 202 response by uploading skin data.
   */
  private static void sendJoinRequestWithSkinHandling(String playerName, String jsonBody,
      SkinUtil.SkinInfo skinInfo) throws URISyntaxException {
    OxeyeConfig config = OxeyeMod.CONFIG;
    URI uri = getBaseUri().resolve("/join");

    HttpRequest req = HttpRequest.newBuilder()
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer " + config.getApiToken())
        .POST(HttpRequest.BodyPublishers.ofString(jsonBody))
        .build();

    client.sendAsync(req, HttpResponse.BodyHandlers.ofString())
        .thenAccept(response -> {
          checkBootIdChange(response);

          if (response.statusCode() == 200) {
            OxeyeMod.LOGGER.info("Join request succeeded for " + playerName + " (skin already known)");
          } else if (response.statusCode() == 202 && skinInfo != null) {
            // Backend needs the skin data - upload it
            OxeyeMod.LOGGER.info("Backend needs skin data for " + playerName + ", uploading...");
            uploadSkinData(skinInfo);
          } else if (response.statusCode() == 202) {
            OxeyeMod.LOGGER.warn("Backend requested skin but no skin info available for " + playerName);
          } else {
            OxeyeMod.LOGGER.error("Join request failed for " + playerName + ": " + parseErrorMessage(response));
          }
        })
        .exceptionally(e -> {
          OxeyeMod.LOGGER.error("Join request failed for " + playerName + ": " + e.getMessage());
          return null;
        });
  }

  /**
   * Upload skin data to the backend.
   */
  private static void uploadSkinData(SkinUtil.SkinInfo skinInfo) {
    // Download the skin PNG
    byte[] skinPng = SkinUtil.downloadSkinPng(skinInfo.textureUrl);
    if (skinPng == null) {
      OxeyeMod.LOGGER.error("Failed to download skin from " + skinInfo.textureUrl);
      return;
    }

    // Encode to base64
    String skinBase64 = java.util.Base64.getEncoder().encodeToString(skinPng);

    // Build the request body
    String json = GSON.toJson(Map.of(
        "texture_hash", skinInfo.textureHash,
        "skin_data", skinBase64,
        "texture_url", skinInfo.textureUrl
    ));

    try {
      sendSkinRequest(json);
    } catch (URISyntaxException e) {
      OxeyeMod.LOGGER.error("Failed to send skin request: " + e.getMessage());
    }
  }

  /**
   * Send skin data to the backend.
   */
  private static void sendSkinRequest(String jsonBody) throws URISyntaxException {
    OxeyeConfig config = OxeyeMod.CONFIG;
    URI uri = getBaseUri().resolve("/skin");

    HttpRequest req = HttpRequest.newBuilder()
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer " + config.getApiToken())
        .POST(HttpRequest.BodyPublishers.ofString(jsonBody))
        .build();

    client.sendAsync(req, HttpResponse.BodyHandlers.ofString())
        .thenAccept(response -> {
          checkBootIdChange(response);
          if (response.statusCode() == 200) {
            OxeyeMod.LOGGER.info("Skin upload succeeded");
          } else {
            OxeyeMod.LOGGER.error("Skin upload failed: " + parseErrorMessage(response));
          }
        })
        .exceptionally(e -> {
          OxeyeMod.LOGGER.error("Skin upload failed: " + e.getMessage());
          return null;
        });
  }

  public static void sendLeaveRequest(String playerName) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending leave request for player: " + playerName);
    postAuthenticatedAsync("/leave", "{\"player\":\"" + playerName + "\"}");
  }

  // ========================================================================
  // Player events (fire-and-forget, uses stored API key)
  // ========================================================================

  public static CompletableFuture<Void> sendSyncRequest(List<String> playerNames) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending sync request for players: " + String.join(", ", playerNames));
    return postAuthenticatedWithFuture("/sync", GSON.toJson(Map.of("players", playerNames)));
  }

  public static CompletableFuture<String> sendConnectRequest(String code) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending connect request with code: " + code);
    URI uri = getBaseUri().resolve("/connect");

    HttpRequest req = HttpRequest.newBuilder()
        .uri(uri)
        .header("Content-Type", "application/json")
        .POST(HttpRequest.BodyPublishers.ofString("{\"code\":\"" + code + "\"}"))
        .build();

    return client.sendAsync(req, HttpResponse.BodyHandlers.ofString())
        .thenApply(response -> {
          if (response.statusCode() == 201) {
            ConnectResponse connectResponse = GSON.fromJson(response.body(), ConnectResponse.class);
            OxeyeMod.LOGGER.info("Connect request succeeded, received API key");
            return connectResponse.api_key;
          } else {
            throw new RuntimeException(parseErrorMessage(response));
          }
        });
  }

  public static CompletableFuture<Void> sendDisconnectSelfRequest() throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending disconnect self request");
    OxeyeConfig config = OxeyeMod.CONFIG;
    String apiToken = config.getApiToken();
    if (apiToken == null || apiToken.isEmpty()) {
      throw new URISyntaxException("", "Not connected - no API key configured");
    }

    URI uri = getBaseUri().resolve("/disconnect");

    HttpRequest req = HttpRequest.newBuilder()
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer " + apiToken)
        .POST(HttpRequest.BodyPublishers.noBody())
        .build();

    return client.sendAsync(req, HttpResponse.BodyHandlers.ofString())
        .thenApply(response -> {
          if (response.statusCode() == 200) {
            OxeyeMod.LOGGER.info("Disconnect request succeeded");
            return null;
          } else {
            throw new RuntimeException(parseErrorMessage(response));
          }
        });
  }

  public static CompletableFuture<Integer> sendStatusRequest() throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending status request");
    URI uri = getBaseUri().resolve("/status");

    OxeyeConfig config = OxeyeMod.CONFIG;
    String apiToken = config.getApiToken();

    HttpRequest.Builder reqBuilder = HttpRequest.newBuilder()
        .uri(uri)
        .GET();

    // Add auth header if we have a token
    if (apiToken != null && !apiToken.isEmpty()) {
      reqBuilder.header("Authorization", "Bearer " + apiToken);
    }

    return client.sendAsync(reqBuilder.build(), HttpResponse.BodyHandlers.ofString())
        .thenApply(HttpResponse::statusCode);
  }

  // ========================================================================
  // Server management commands (return futures for feedback)
  // ========================================================================

  private static URI getBaseUri() throws URISyntaxException {
    OxeyeConfig config = OxeyeMod.CONFIG;
    if (config.getServerUrl() == null) {
      OxeyeMod.LOGGER.error("Server URL is not configured.");
      throw new URISyntaxException("", "Server URL is not configured.");
    }
    return config.getServerUrl().toURI();
  }

  private static String parseErrorMessage(HttpResponse<String> response) {
    try {
      ErrorResponse errorResponse = GSON.fromJson(response.body(), ErrorResponse.class);
      if (errorResponse.error != null) {
        return errorResponse.error;
      }
    } catch (Exception ignored) {
    }
    return "Unknown error (status " + response.statusCode() + ")";
  }

  // ========================================================================
  // Helper methods
  // ========================================================================

  private static CompletableFuture<Void> postAuthenticatedWithFuture(String endpoint, String jsonBody) throws URISyntaxException {
    OxeyeConfig config = OxeyeMod.CONFIG;
    URI uri = getBaseUri().resolve(endpoint);

    HttpRequest req = HttpRequest.newBuilder()
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer " + config.getApiToken())
        .POST(HttpRequest.BodyPublishers.ofString(jsonBody))
        .build();

    return client.sendAsync(req, HttpResponse.BodyHandlers.ofString())
        .thenAccept(response -> {
          checkBootIdChange(response);
          if (response.statusCode() >= 200 && response.statusCode() < 300) {
            OxeyeMod.LOGGER.info("HTTP request to " + endpoint + " succeeded");
          } else {
            OxeyeMod.LOGGER.error("HTTP request to " + endpoint + " failed: " + parseErrorMessage(response));
          }
        })
        .exceptionally(e -> {
          OxeyeMod.LOGGER.error("HTTP request to " + endpoint + " failed: " + e.getMessage());
          return null;
        });
  }

  private static void postAuthenticatedAsync(String endpoint, String jsonBody) throws URISyntaxException {
    OxeyeConfig config = OxeyeMod.CONFIG;
    URI uri = getBaseUri().resolve(endpoint);

    HttpRequest req = HttpRequest.newBuilder()
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer " + config.getApiToken())
        .POST(HttpRequest.BodyPublishers.ofString(jsonBody))
        .build();

    client.sendAsync(req, HttpResponse.BodyHandlers.ofString())
        .thenAccept(response -> {
          checkBootIdChange(response);
          if (response.statusCode() >= 200 && response.statusCode() < 300) {
            OxeyeMod.LOGGER.info("HTTP request to " + endpoint + " succeeded");
          } else {
            OxeyeMod.LOGGER.error("HTTP request to " + endpoint + " failed: " + parseErrorMessage(response));
          }
        })
        .exceptionally(e -> {
          OxeyeMod.LOGGER.error("HTTP request to " + endpoint + " failed: " + e.getMessage());
          return null;
        });
  }

  private static void checkBootIdChange(HttpResponse<String> response) {
    String bootId = response.headers().firstValue("X-Boot-ID").orElse(null);
    if (bootId != null && !bootId.equals(lastBootId)) {
      OxeyeMod.LOGGER.warn("Boot ID changed from " + lastBootId + " to " + bootId + ", backend restarted");
      lastBootId = bootId;
      // Trigger a sync on next server tick to resync state
      SyncManager.markOutOfSync();
    } else if (bootId != null) {
      lastBootId = bootId;
    }
  }

  public static String getLastBootId() {
    return lastBootId;
  }

  // Response types for JSON parsing
  private static class ErrorResponse {
    String error;
    String details;
  }

  private static class ConnectResponse {
    String api_key;
  }
}
