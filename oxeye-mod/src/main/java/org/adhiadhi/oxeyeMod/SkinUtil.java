package org.adhiadhi.oxeyeMod;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import com.mojang.authlib.GameProfile;
import com.mojang.authlib.properties.Property;

import java.io.ByteArrayOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.util.Base64;
import java.util.Optional;

/**
 * Utility class for extracting skin data from Minecraft GameProfiles.
 */
public class SkinUtil {
    private static final HttpClient HTTP_CLIENT = HttpClient.newHttpClient();

    /**
     * Represents extracted skin information from a GameProfile.
     */
    public static class SkinInfo {
        public final String textureHash;
        public final String textureUrl;
        public final String textureValue;  // Raw base64-encoded texture property value

        public SkinInfo(String textureHash, String textureUrl, String textureValue) {
            this.textureHash = textureHash;
            this.textureUrl = textureUrl;
            this.textureValue = textureValue;
        }
    }

    /**
     * Extract skin information from a player's GameProfile.
     *
     * @param profile The player's GameProfile
     * @return Optional containing SkinInfo if textures are available, empty otherwise
     */
    public static Optional<SkinInfo> extractSkinInfo(GameProfile profile) {
        // Get the textures property (GameProfile is a record in authlib 7.x)
        var texturesProperties = profile.properties().get("textures");
        if (texturesProperties == null || texturesProperties.isEmpty()) {
            OxeyeMod.LOGGER.debug("No textures property found for player: {}", profile.name());
            return Optional.empty();
        }

        Property texturesProperty = texturesProperties.iterator().next();
        String textureValue = texturesProperty.value();

        // Compute SHA256 hash of the texture value
        String textureHash = sha256(textureValue);

        // Decode and parse the texture JSON to get the skin URL
        try {
            String json = new String(Base64.getDecoder().decode(textureValue), StandardCharsets.UTF_8);
            JsonObject root = JsonParser.parseString(json).getAsJsonObject();
            JsonObject textures = root.getAsJsonObject("textures");

            if (textures == null || !textures.has("SKIN")) {
                OxeyeMod.LOGGER.debug("No SKIN texture found for player: {}", profile.name());
                return Optional.empty();
            }

            String skinUrl = textures.getAsJsonObject("SKIN").get("url").getAsString();

            OxeyeMod.LOGGER.debug("Extracted skin info for {}: hash={}, url={}",
                    profile.name(), textureHash.substring(0, 16) + "...", skinUrl);

            return Optional.of(new SkinInfo(textureHash, skinUrl, textureValue));
        } catch (Exception e) {
            OxeyeMod.LOGGER.error("Failed to parse texture JSON for player {}: {}",
                    profile.name(), e.getMessage());
            return Optional.empty();
        }
    }

    /**
     * Download skin PNG data from a URL.
     *
     * @param skinUrl The URL to download from
     * @return The PNG bytes, or null if download failed
     */
    public static byte[] downloadSkinPng(String skinUrl) {
        try {
            HttpRequest request = HttpRequest.newBuilder()
                    .uri(URI.create(skinUrl))
                    .GET()
                    .build();

            HttpResponse<InputStream> response = HTTP_CLIENT.send(request,
                    HttpResponse.BodyHandlers.ofInputStream());

            if (response.statusCode() != 200) {
                OxeyeMod.LOGGER.error("Failed to download skin, status: {}", response.statusCode());
                return null;
            }

            // Read all bytes
            try (InputStream is = response.body();
                 ByteArrayOutputStream baos = new ByteArrayOutputStream()) {
                byte[] buffer = new byte[8192];
                int bytesRead;
                while ((bytesRead = is.read(buffer)) != -1) {
                    baos.write(buffer, 0, bytesRead);
                }
                return baos.toByteArray();
            }
        } catch (IOException | InterruptedException e) {
            OxeyeMod.LOGGER.error("Failed to download skin from {}: {}", skinUrl, e.getMessage());
            return null;
        }
    }

    /**
     * Compute SHA256 hash of a string.
     *
     * @param input The string to hash
     * @return Lowercase hex string representation of the hash
     */
    public static String sha256(String input) {
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            byte[] hash = digest.digest(input.getBytes(StandardCharsets.UTF_8));
            StringBuilder hexString = new StringBuilder();
            for (byte b : hash) {
                String hex = Integer.toHexString(0xff & b);
                if (hex.length() == 1) {
                    hexString.append('0');
                }
                hexString.append(hex);
            }
            return hexString.toString();
        } catch (NoSuchAlgorithmException e) {
            throw new RuntimeException("SHA-256 not available", e);
        }
    }
}
