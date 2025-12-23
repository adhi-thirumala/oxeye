package org.adhiadhi.oxeyeMod;

import com.google.gson.Gson;

import java.net.URI;
import java.net.URISyntaxException;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.util.ArrayList;
import java.util.List;
import java.util.concurrent.CompletableFuture;

public class OxeyeHttp {
  private static final HttpClient client = HttpClient.newHttpClient();
  private static final Gson GSON = new Gson();

  public static void sendJoinRequest(String playerName) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending join request for player: " + playerName);
    postAuthenticatedAsync("/join", "{\"player\":\"" + playerName + "\"}");
  }

  public static void sendLeaveRequest(String playerName) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending leave request for player: " + playerName);
    postAuthenticatedAsync("/leave", "{\"player\":\"" + playerName + "\"}");
  }

  // ========================================================================
  // Player events (fire-and-forget, uses stored API key)
  // ========================================================================

  public static void sendSyncRequest(List<String> playerNames) throws URISyntaxException {
    OxeyeMod.LOGGER.info("Sending sync request for players: " + String.join(", ", playerNames));
    postAuthenticatedAsync("/sync", GSON.toJson(new Object() {
      final List<String> players = playerNames;
    }));
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

  // Response types for JSON parsing
  private static class ErrorResponse {
    String error;
    String details;
  }

  private static class ConnectResponse {
    String api_key;
  }
}
