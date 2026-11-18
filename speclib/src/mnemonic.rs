use std::cmp::Ordering;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::{Hash, Hasher};

use serde::{Deserialize, Serialize};
use serde::de::Error as DeError;

/// Word shown when a byte somehow misses the table.
const UNLISTED: &str = "unlisted";

const WORDS: [&str; 256] = [
"acid","amber","anchor","apple","arrow","ash","aspen","aurora","autumn","avenue","badge","bamboo","banjo","basalt","beacon","beech","bell","birch","bishop","bloom","boulder","bracelet","brass","bridge","bronze","brook","brush","cabin","cactus","canyon","canvas","cedar","chalk","chapel","charcoal","cherry","chime","cinder","citrus","clover","cobalt","comet","compass","copper","coral","cosmos","cotton","crater","crayon","creek","crest","crimson","crystal","cupola","cypress","dahlia","dam","delta","denim","dew","diamond","dock","domino","dove","drift","drum","dune","dusk","ember","emerald","engine","eucalyptus","fable","falcon","fallow","felt","fern","fiber","fig","filament","finch","fir","fjord","flame","flax","flint","flora","flute","fog","forest","fossil","fountain","fox","fresco","frost","garnet","gazebo","gecko","geyser","ginger","glacier","glass","glen","granite","gravel","grove","gulf","gully","gusto","hammock","harbor","hazel","heather","hemlock","hickory","hollow","horizon","horn","hurdle","ibis","indigo","ink","iris","ivory","jasmine","jasper","jetty","jewel","juniper","kayak","kernel","kettle","kiln","kingfisher","kite","kitten","lace","lake","lantern","larch","lattice","lava","ledger","lemon","lens","lichen","lilac","lily","linen","lintel","lodge","lotus","lumber","lure","lyre","magnet","mahogany","maple","marble","marlin","marsh","meadow","mesa","mica","mint","mist","monsoon","moon","moss","moth","mulberry","murmur","museum","myrtle","nebula","nectar","needle","nettle","nickel","nimbus","north","nurse","oak","oasis","obsidian","ocean","ochre","onyx","opal","orbit","orchard","orchid","ore","osprey","ottoman","oyster","paddle","pagoda","palm","paper","pebble","pecan","pewter","phoenix","pigment","pine","pitch","pith","plateau","plum","plume","poem","pollen","pond","porch","portal","prairie","prism","pumice","quartz","quill","quiver","raccoon","rain","rapids","raspberry","raven","reed","reef","relay","resin","ridge","rift","ring","ripple","river","robin","rocket","rose","rubble","ruby","rush","saffron","sage","salmon","sand","sapphire","satchel","scaffold","scarf","sedge","sequoia","shade","shadow","shell","shore"
];

pub struct Mnemonic(pub String);

pub enum MnemonicError {
    UnknownWord { word: String },
    Empty,
}

impl Debug for MnemonicError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownWord { word } => write!(f, "unknown word \"{word}\""),
            Self::Empty => f.write_str("empty mnemonic"),
        }
    }
}

impl Display for MnemonicError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

impl Error for MnemonicError {}

impl Mnemonic {
    #[must_use]
    pub fn from_entropy(entropy: &[u8]) -> Self {
        assert!(!entropy.is_empty(), "entropy must be non-empty");
        Self(
            entropy
                .iter()
                .map(|byte| WORDS.get(usize::from(*byte)).copied().unwrap_or(UNLISTED))
                .collect::<Vec<&str>>()
                .join("-"),
        )
    }

    pub fn parse(source: &str) -> Result<Self, MnemonicError> {
        let trimmed = source.trim();
        if trimmed.is_empty() {
            return Err(MnemonicError::Empty);
        }
        for word in trimmed.replace('-', " ").split_whitespace() {
            if !WORDS.contains(&word) {
                return Err(MnemonicError::UnknownWord { word: word.to_owned() });
            }
        }
        Ok(Self(trimmed.to_owned()))
    }
}

impl Display for Mnemonic {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Debug for Mnemonic {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl PartialEq for Mnemonic {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl Eq for Mnemonic {}

impl Hash for Mnemonic {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.hash(state);
    }
}

impl Clone for Mnemonic {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl Serialize for Mnemonic {
    fn serialize<Serializer: serde::Serializer>(&self, serializer: Serializer) -> Result<Serializer::Ok, Serializer::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Mnemonic {
    fn deserialize<De: serde::Deserializer<'de>>(deserializer: De) -> Result<Self, De::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(|error| DeError::custom(error.to_string()))
    }
}

impl Ord for Mnemonic {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Mnemonic {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
