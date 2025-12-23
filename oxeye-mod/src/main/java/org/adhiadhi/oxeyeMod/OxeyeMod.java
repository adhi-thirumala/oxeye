package org.adhiadhi.oxeyeMod;


import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.networking.v1.ServerPlayConnectionEvents;
import org.slf4j.LoggerFactory;

import java.net.MalformedURLException;
import java.util.logging.Logger;


public class OxeyeMod implements ModInitializer {
  public static final Logger LOGGER = (Logger) LoggerFactory.getLogger("oxeye-mod");
  public static OxeyeConfig CONFIG;

  @Override
  public void onInitialize() {
    try {
      CONFIG = OxeyeConfig.load();
    } catch (MalformedURLException e) {
      throw new RuntimeException(e);
    }
    LOGGER.info("Oxeye Config initialized");
    ServerPlayConnectionEvents.JOIN.register(OxeyeEvents::onPlayerJoin);
    ServerPlayConnectionEvents.DISCONNECT.register(OxeyeEvents::onPlayerDisconnect);
    ServerLifecycleEvents.SERVER_STARTED.register(OxeyeEvents::onServerStarted);
    ServerLifecycleEvents.SERVER_STOPPED.register(OxeyeEvents::onServerStopped);
    CommandRegistrationCallback.EVENT.register(OxeyeCommands::register);
  }
}
