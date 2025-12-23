package org.adhiadhi.oxeyeMod;

import com.google.gson.Gson;
import com.google.gson.GsonBuilder;
import net.fabricmc.loader.api.FabricLoader;

import java.net.MalformedURLException;
import java.net.URL;
import java.nio.file.Path;
import java.util.logging.Level;
import java.util.logging.Logger;

public class OxeyeConfig {
    private static final Path CONFIG_PATH =
            FabricLoader.getInstance().getConfigDir().resolve("oxeye.json");
    private static final Gson GSON = new GsonBuilder().setPrettyPrinting().create();
    private String api_token = null;
    private URL server_url = new URL("https://oxeye.adhithirumala.com");

    public OxeyeConfig() throws MalformedURLException {
    }

    public static OxeyeConfig load() throws MalformedURLException {
        if (java.nio.file.Files.exists(CONFIG_PATH)) {
            try {
                String json = java.nio.file.Files.readString(CONFIG_PATH);
                return GSON.fromJson(json, OxeyeConfig.class);
            } catch (Exception e) {
                Logger.getGlobal().log(Level.CONFIG, "Failed to read config file: " + e.getMessage());
            }
        }
        OxeyeConfig config = new OxeyeConfig();
        config.save();
        return config;
    }

    public void save() {
        try {
            java.nio.file.Files.writeString(CONFIG_PATH, GSON.toJson(this));
        } catch (Exception e) {
            Logger.getGlobal().log(Level.CONFIG, "Failed to write config file: " + e);
        }
    }


    public String getApiToken() {
        return api_token;
    }

    public void setApiToken(String apiToken) {
        this.api_token = apiToken;
    }

    public URL getServerUrl() {
        return server_url;
    }
}
