package org.adhiadhi.oxeyeMod;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.StringArgumentType;
import com.mojang.brigadier.context.CommandContext;
import net.minecraft.commands.CommandBuildContext;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.minecraft.server.permissions.Permissions;

import java.net.URISyntaxException;

public class OxeyeCommands {

  public static void register(CommandDispatcher<CommandSourceStack> dispatcher, CommandBuildContext registryAccess, Commands.CommandSelection environment) {
    dispatcher.register(Commands.literal("oxeye")
        .then(Commands.literal("connect")
            .requires(source -> source.permissions().hasPermission(Permissions.COMMANDS_OWNER))
            .then(Commands.argument("code", StringArgumentType.string())
                .executes(OxeyeCommands::connect)))
        .then(Commands.literal("disconnect")
            .requires(source -> source.permissions().hasPermission(Permissions.COMMANDS_OWNER))
            .executes(OxeyeCommands::disconnect))
        .then(Commands.literal("status")
            .executes(OxeyeCommands::status)));
  }

  private static int connect(CommandContext<CommandSourceStack> context) {
    CommandSourceStack source = context.getSource();
    String code = StringArgumentType.getString(context, "code");

    // Check if already connected
    String existingToken = OxeyeMod.CONFIG.getApiToken();
    if (existingToken != null && !existingToken.isEmpty()) {
      source.sendFailure(Component.literal("Already connected. Run /oxeye disconnect first to connect to a different server."));
      return 1;
    }

    try {
      OxeyeHttp.sendConnectRequest(code)
          .thenAccept(apiKey -> {
            OxeyeMod.CONFIG.setApiToken(apiKey);
            OxeyeMod.CONFIG.save();
            source.sendSuccess(() -> Component.literal("Connected successfully! API key saved."), false);
          })
          .exceptionally(e -> {
            sendError(source, e);
            return null;
          });
    } catch (URISyntaxException e) {
      source.sendFailure(Component.literal(e.getReason()));
      return 1;
    }

    return 0;
  }

  private static int disconnect(CommandContext<CommandSourceStack> context) {
    CommandSourceStack source = context.getSource();

    // Check if not connected
    String existingToken = OxeyeMod.CONFIG.getApiToken();
    if (existingToken == null || existingToken.isEmpty()) {
      source.sendFailure(Component.literal("Not connected. Nothing to disconnect."));
      return 1;
    }

    try {
      OxeyeHttp.sendDisconnectSelfRequest()
          .thenAccept(v -> {
            OxeyeMod.CONFIG.setApiToken(null);
            OxeyeMod.CONFIG.save();
            source.sendSuccess(() -> Component.literal("Disconnected successfully. API key removed."), false);
          })
          .exceptionally(e -> {
            sendError(source, e);
            return null;
          });
    } catch (URISyntaxException e) {
      source.sendFailure(Component.literal(e.getReason()));
      return 1;
    }

    return 0;
  }

  private static int status(CommandContext<CommandSourceStack> context) {
    CommandSourceStack source = context.getSource();

    try {
      OxeyeHttp.sendStatusRequest()
          .thenAccept(statusCode -> {
            if (statusCode == 200) {
              source.sendSuccess(() -> Component.literal("Backend is up and authenticated."), false);
            } else if (statusCode == 401) {
              source.sendSuccess(() -> Component.literal("Backend is up, but not authenticated (invalid or missing API key)."), false);
            } else {
              source.sendSuccess(() -> Component.literal("Backend is up (status " + statusCode + ")."), false);
            }
          })
          .exceptionally(e -> {
            source.sendFailure(Component.literal("Backend is unreachable: " + e.getCause().getMessage()));
            return null;
          });
    } catch (URISyntaxException e) {
      source.sendFailure(Component.literal(e.getReason()));
      return 1;
    }

    return 0;
  }

  private static void sendError(CommandSourceStack source, Throwable e) {
    String message = e.getCause() != null ? e.getCause().getMessage() : e.getMessage();
    source.sendFailure(Component.literal(message));
  }
}
