//! Curated official news feeds and Wikipedia titles for popular non-Steam games.
//! Admin overrides on `game_page_meta` still win. Only feeds that are real
//! public RSS/Atom on official (or official-community) hosts are listed.

/// Official (or official-community) RSS/Atom URL for a catalogue game id.
pub fn official_news_url(game_id: &str) -> Option<&'static str> {
    Some(match game_id {
        // Roblox posts product updates on the official Developer Forum.
        "roblox" => "https://devforum.roblox.com/c/updates/45.rss",
        _ => return None,
    })
}

/// Preferred English Wikipedia title for a catalogue game id, when the
/// catalogue name would be ambiguous or would miss the article.
pub fn wiki_title(game_id: &str) -> Option<&'static str> {
    Some(match game_id {
        "minecraft_bedrock" => "Minecraft",
        "minecraft_java" => "Minecraft (Java Edition)",
        "roblox" => "Roblox",
        "lol" => "League of Legends",
        "valorant" => "Valorant (video game)",
        "fortnite" => "Fortnite",
        "genshin" => "Genshin Impact",
        "hsr" => "Honkai: Star Rail",
        "hearthstone" => "Hearthstone",
        "wow" => "World of Warcraft",
        "wow_classic" => "World of Warcraft",
        "star_citizen" => "Star Citizen",
        "osu" => "Osu!",
        "diablo" => "Diablo (video game)",
        "diablo2" => "Diablo II",
        "diablo3" => "Diablo III",
        "diablo_immortal" => "Diablo Immortal",
        "sc2" => "StarCraft II",
        "starcraft" => "StarCraft (video game)",
        "hots" => "Heroes of the Storm",
        "2xko" => "2XKO",
        "maplestory" => "MapleStory",
        "tibia" => "Tibia (video game)",
        "ragnarok_online" => "Ragnarok Online",
        "lineage_ii" => "Lineage II",
        "crossfire" => "CrossFire (video game)",
        "escape_from_tarkov_arena" => "Escape from Tarkov",
        "goddess_of_victory_nikke" => "Goddess of Victory: Nikke",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curated_map_covers_popular_non_steam() {
        assert_eq!(wiki_title("minecraft_bedrock"), Some("Minecraft"));
        assert_eq!(wiki_title("roblox"), Some("Roblox"));
        assert_eq!(wiki_title("lol"), Some("League of Legends"));
        assert_eq!(
            official_news_url("roblox"),
            Some("https://devforum.roblox.com/c/updates/45.rss")
        );
        // A distinct product — do not silently reuse retail WoW's article.
        assert_eq!(wiki_title("wow_forever"), None);
        assert_eq!(official_news_url("wow_forever"), None);
        assert_eq!(official_news_url("minecraft_bedrock"), None);
    }
}
