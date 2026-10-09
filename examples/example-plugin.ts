/*
 * Example Vencord plugin, used to demonstrate `dcrs-port plugin`.
 * The interesting part is what it touches: clean API calls, plus one declarative
 * patch that no native client can replicate.
 */

import { definePlugin } from "Vencord";
import { findByProps } from "webpack";

export default definePlugin({
    name: "ExamplePlugin",
    description: "Exercises every surface class for the porting report.",
    authors: [{ name: "someone", id: BigInt(1234567890) }],
    tags: ["Utility", "Chat"],

    // Class (e): string-replaces a minified module factory and re-evals it.
    // This is the irreducible part. No native client can port it.
    patches: [
        {
            find: "analytics:void 0",
            replacement: "analytics:()=>{}",
            all: true,
        },
        {
            find: "sentry.init",
            replacement: "sentry.init",
            noWarn: true,
        },
    ],

    start() {
        // Class (a): data reads.
        const channel = Vencord.Webpack.Common.ChannelStore.getChannel(this.channelId);
        const members = Vencord.Webpack.Common.GuildMemberStore.getMemberCount(this.guildId);

        // Class (b): data writes.
        FluxDispatcher.dispatch({ type: "AUDIO_TOGGLE_SELF_MUTE" });

        // Class (c): subscriptions.
        FluxDispatcher.subscribe("MESSAGE_CREATE", this.onMessage);
        MediaEngineStore.engine.on("DeviceChange", () => {
            /* re-render device pickers */
        });

        // Class (e): internals, via the BetterDiscord-style runtime patcher.
        const mod = findByProps("someInternalProp");
        Patcher.before(mod, "render", function (args) {
            args[0].hidden = true;
        });

        DataStore.set("examplePlugin.enabled", true);
    },

    stop() {
        DataStore.del("examplePlugin.enabled");
        Patcher.unpatchAll();
    },

    // Class (d): UI injection. Free for a native client, since it owns the render tree.
    renderMessageAccessory(message) {
        return null;
    },

    chatBarButton() {
        return null;
    },
});
