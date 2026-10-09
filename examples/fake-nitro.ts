/*
 * FakeNitro, reduced from Vendicated/Vencord `src/plugins/fakeNitro/index.tsx`.
 *
 * Every `find:` and `match:` string here is verbatim from upstream, because the point of the
 * `effects` subcommand is to show that a plugin made entirely of webpack patches is really a set
 * of capability gates plus a message-rewrite pass.
 *
 * Used with: dcrs-port effects examples/fake-nitro.ts
 */
export default definePlugin({
    name: "FakeNitro",
    description: "Allows you to send fake emojis/stickers, use nitro themes, and stream in nitro quality",
    tags: ["Emotes", "Appearance", "Customisation", "Chat"],

    settings: definePluginSettings({
        enableEmojiBypass: { type: OptionType.BOOLEAN, default: true, restartNeeded: true },
        enableStickerBypass: { type: OptionType.BOOLEAN, default: true, restartNeeded: true },
        enableStreamQualityBypass: { type: OptionType.BOOLEAN, default: true, restartNeeded: true },
    }),

    patches: [
        {
            find: "canUseCustomStickersEverywhere:",
            replacement: [
                {
                    match: /(?<=canUseCustomStickersEverywhere:function\(\i\)\{)/,
                    replace: "return true;",
                    predicate: () => settings.store.enableStickerBypass,
                },
                {
                    match: /(?<=canUseHighVideoUploadQuality:function\(\i\)\{)/,
                    replace: "return true;",
                    predicate: () => settings.store.enableStreamQualityBypass,
                },
                {
                    match: /(?<=canStreamQuality:function\(\i,\i\)\{)/,
                    replace: "return true;",
                    predicate: () => settings.store.enableStreamQualityBypass,
                },
                {
                    match: /(?<=canUseClientThemes:function\(\i\)\{)/,
                    replace: "return true;",
                },
                {
                    match: /(?<=canUsePremiumAppIcons:function\(\i\)\{)/,
                    replace: "return true;",
                },
            ],
        },
        {
            find: '.getByName("fork_and_knife")',
            predicate: () => settings.store.enableEmojiBypass,
            replacement: { match: ".CHAT", replace: ".STATUS" },
        },
        {
            find: ".GUILD_SUBSCRIPTION_UNAVAILABLE;",
            group: true,
            predicate: () => settings.store.enableEmojiBypass,
            replacement: [
                { match: /(?<=\.USE_EXTERNAL_EMOJIS.+?;)(?<=intention:(\i).+?)/, replace: "const fakeNitroIntention=$1;" },
                { match: /\.available\?/, replace: "true?" },
            ],
        },
        { find: ".getUserIsAdmin(", replacement: { match: /function \i\(\i,\i\)\)/, replace: "function $(i, fakeNitroOriginal)" } },
        { find: '"SENDABLE"', predicate: () => settings.store.enableStickerBypass, replacement: { match: /\i\.available\?/, replace: "true?" } },
        { find: "#{intl::STREAM_FPS_OPTION}", predicate: () => settings.store.enableStreamQualityBypass, replacement: { match: /guildPremiumTier:\i\.\i\.TIER_\d,?/g, replace: "" } },
        { find: '"UserSettingsProtoStore"', replacement: { match: /CONNECTION_OPEN/, replace: "$self.handleProtoChange" } },
        { find: ",updateTheme(", replacement: { match: /(function \i\(\i\))/ , replace: "$self.handleGradientThemeSelect" } },
        { find: ".CLIENT_THEMES_EDITOR?", replacement: { match: /\(0,\i\.\i\)\(\i\.\i\.TIER_2\)/g, replace: "true" } },
        { find: "getCurrentDesktopIcon(),", replacement: { match: /\i\.\i\.isPremium\(\i\.\i\.getCurrentUser\(\)\)/, replace: "true" } },
        { find: 'type:"GUILD_SOUNDBOARD_SOUND_CREATE"', replacement: { match: /\.available/g, replace: "true" } },
    ],

    start() {
        this.preSend = addMessagePreSendListener(async (channelId, messageObj, options) => {
            /* rewrite premium emoji and stickers into links */
        });
        this.preEdit = addMessagePreEditListener(async (channelId, __, messageObj) => {
            /* same, on the edit path */
        });
        if (!hasEmbedPerms(channelId)) showCannotEmbedNotice();
        sendAnimatedSticker(link, sticker.id, channelId);
    },
});