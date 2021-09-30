use std::borrow::Cow;

use regex::{Captures, Regex};
use serenity::{
    builder::{CreateEmbed, CreateEmbedAuthor, CreateEmbedFooter, CreateMessage},
    model::channel::{Embed, EmbedAuthor, EmbedField, EmbedFooter},
};

use super::prelude::*;

use utility::regex_lazy;

static SENTENCE_RGX: once_cell::sync::Lazy<Regex> = regex_lazy!(
    r#"(?msx)                                                               # Flags
        (?P<text>.*?[\w&&[^_]]+.*?)                                             # Text, not including underscores at the end.
        (?P<punct>
            [\.!\?\u3002\u0629\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]+     # Match punctuation not at the end of a line.
            |
        \s*(?:                                                                  # Include eventual whitespace after peko.
                [\.!\?\u3002\u0629\uFE12\uFE52\uFF0E\uFF61\uFF01\uFF1F"_\*`\)]  # Match punctuation at the end of a line.
                |
                (?:<a?:\w+:\d+>)                                                # Match Discord emotes at the end of a line.
                |
                [\x{1F600}-\x{1F64F}]                                           # Match Unicode emoji at the end of a line.
            )*$
        )"#
);

#[command]
#[allowed_roles(
    "Admin",
    "Moderator",
    "Moderator (JP)",
    "Server Booster",
    "40 m deep",
    "50 m deep",
    "60 m deep",
    "70 m deep",
    "80 m deep",
    "90 m deep",
    "100 m deep"
)]
/// Pekofies replied-to message or the provided text.
pub async fn pekofy(ctx: &Ctx, msg: &Message) -> CommandResult {
    let mut reply = CreateMessage::default();

    let (text, embeds) = match get_data(ctx, msg).await? {
        Some((text, embeds)) => (text, embeds),
        None => return Ok(()),
    };

    let mut result = Ok(());

    if let Some(text) = text {
        result = result.and(if text.starts_with("-pekofy") {
            Err(anyhow!("Nice try peko"))
        } else {
            pekofy_text(&text).map(|text| {
                reply.content(text);
            })
        });
    }

    if !embeds.is_empty() {
        result = result.and(pekofy_embeds(msg, &mut reply).await);
    }

    if let Err(e) = result {
        msg.channel_id
            .say(&ctx.http, e.to_string())
            .await
            .context(here!())?;

        return Ok(());
    }

    msg.channel_id
        .send_message(&ctx.http, |_| &mut reply)
        .await
        .context(here!())?;

    Ok(())
}

#[inline]
pub fn pekofy_text(text: &str) -> anyhow::Result<String> {
    let pekofied_text = SENTENCE_RGX.replace_all(text, |capture: &Captures| -> Cow<str> {
        // Check if the capture is empty.
        if capture
            .get(0)
            .map(|c| c.as_str().trim().is_empty())
            .unwrap_or(true)
        {
            return Cow::Borrowed(text);
        }

        let text = capture
            .name("text")
            .map(|m| m.as_str())
            .unwrap_or_else(|| "Couldn't find 'text' capture!");

        let response = match get_peko_response(text) {
            Ok(response) => response,
            Err(_) => return Cow::Owned(text.to_owned()),
        };

        Cow::Owned(format!(
            "{}{}{}",
            text,
            response,
            capture.name("punct").map(|m| m.as_str()).unwrap_or("")
        ))
    });

    Ok(pekofied_text.into_owned())
}

async fn pekofy_embeds(msg: &Message, reply: &mut CreateMessage<'_>) -> anyhow::Result<()> {
    reply.set_embeds(
        msg.embeds
            .iter()
            .map(|e| pekofy_embed(e))
            .collect::<anyhow::Result<_>>()?,
    );

    Ok(())
}

fn pekofy_embed(embed: &Embed) -> anyhow::Result<CreateEmbed> {
    let mut peko_embed = CreateEmbed::default();

    if let Some(EmbedAuthor {
        name,
        icon_url,
        url,
        ..
    }) = &embed.author
    {
        let mut peko_author = CreateEmbedAuthor::default();

        peko_author.name(pekofy_text(name)?);

        if let Some(icon_url) = icon_url {
            peko_author.icon_url(icon_url);
        }

        if let Some(url) = url {
            peko_author.url(url);
        }

        peko_embed.set_author(peko_author);
    }

    if let Some(EmbedFooter { text, icon_url, .. }) = &embed.footer {
        let mut peko_footer = CreateEmbedFooter::default();

        peko_footer.text(pekofy_text(text)?);

        if let Some(icon_url) = icon_url {
            peko_footer.icon_url(icon_url);
        }

        peko_embed.set_footer(peko_footer);
    }

    if let Some(title) = &embed.title {
        peko_embed.title(pekofy_text(title)?);
    }

    if let Some(description) = &embed.description {
        peko_embed.description(pekofy_text(description)?);
    }

    if !embed.fields.is_empty() {
        peko_embed.fields(
            embed
                .fields
                .iter()
                .map(
                    |EmbedField {
                         name,
                         value,
                         inline,
                         ..
                     }| {
                        match [pekofy_text(name), pekofy_text(value)] {
                            [Ok(name), Ok(value)] => Ok((name, value, *inline)),
                            [Err(n), Err(v)] => Err(n).context(v),
                            [Err(e), _] | [_, Err(e)] => Err(e),
                        }
                    },
                )
                .collect::<anyhow::Result<Vec<_>>>()?,
        );
    }

    Ok(peko_embed)
}

#[allow(clippy::needless_lifetimes)]
async fn get_data<'a>(
    ctx: &Ctx,
    msg: &'a Message,
) -> anyhow::Result<Option<(Option<String>, &'a Vec<Embed>)>> {
    let mut args = Args::new(
        &msg.content_safe(&ctx.cache).await,
        &[Delimiter::Single(' ')],
    );
    args.trimmed();
    args.advance();

    let text;
    let embeds;

    if let Some(src) = &msg.referenced_message {
        if src.author.bot {
            return Ok(None);
        }

        embeds = &src.embeds;

        let safe_text = src.content_safe(&ctx.cache).await;

        text = if safe_text.trim().is_empty() {
            if embeds.is_empty() {
                return Ok(None);
            }

            None
        } else {
            Some(safe_text)
        };

        msg.delete(&ctx.http).await.context(here!())?;
    } else {
        embeds = &msg.embeds;

        text = match args.remains() {
            Some(remains) => Some(remains.to_owned()),
            None if embeds.is_empty() => return Ok(None),
            None => None,
        };

        msg.delete(&ctx.http).await.context(here!())?;
    }

    Ok(Some((text, embeds)))
}

#[inline(always)]
fn get_peko_response(text: &str) -> anyhow::Result<&str> {
    let text_is_uppercase = text == text.to_uppercase();

    // Get response based on alphabet used.
    Ok(
        match text
            .chars()
            .last()
            .ok_or_else(|| anyhow!("Can't get last character!"))
            .context(here!())? as u32
        {
            // Greek
            0x0370..=0x03FF => {
                if text_is_uppercase {
                    " ΠΈΚΟ"
                } else {
                    " πέκο"
                }
            }
            // Russian
            0x0400..=0x04FF => {
                if text_is_uppercase {
                    " ПЕКО"
                } else {
                    " пеко"
                }
            }
            // Arabic
            0x0600..=0x06FF => "بيكو ",
            // Georgian
            0x10A0..=0x10FF | 0x1C90..=0x1CBF => " პეკო",
            // Japanese
            0x3040..=0x30FF | 0xFF00..=0xFFEF | 0x4E00..=0x9FAF => "ぺこ",
            // Korean
            0xAC00..=0xD7AF
            | 0x1100..=0x11FF
            | 0xA960..=0xA97F
            | 0xD7B0..=0xD7FF
            | 0x3130..=0x318F => "페코",
            // Latin
            _ => {
                if text_is_uppercase {
                    " PEKO"
                } else {
                    " peko"
                }
            }
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_test::traced_test;

    #[traced_test]
    #[test]
    fn pekofy_empty_string() {
        assert_eq!(pekofy_text("").unwrap(), "");
    }

    #[traced_test]
    #[test]
    fn pekofy_single_emoji() {
        assert_eq!(pekofy_text("🐱").unwrap(), "🐱 peko");
        assert_eq!(
            pekofy_text("<:pekoFeet:828454981836996608>").unwrap(),
            "<:pekoFeet:828454981836996608> peko"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_multiple_emoji() {
        assert_eq!(pekofy_text("🐱🐱").unwrap(), "🐱🐱 peko");
        assert_eq!(pekofy_text("🐱 🐱").unwrap(), "🐱 🐱 peko");
        assert_eq!(
            pekofy_text("😀😁😂🤣😃😄😅😆😉😊😋😎😍😘😗😙😚🙂🤗🤩🤔🤨😐😑😶🙄😏😣😥😮🤐😯😪😫😴😌😛😜😝😒😓😔😕🙃🤑😲😖😞😟😤😢😭😦😧😨😩🤯😬😰😱😳🤪😵😡😠😇😷🤓😎🤖🤗😻😼😽🙀😿😾").unwrap(),
            "😀😁😂🤣😃😄😅😆😉😊😋😎😍😘😗😙😚🙂🤗🤩🤔🤨😐😑😶🙄😏😣😥😮🤐😯😪😫😴😌😛😜😝😒😓😔😕🙃🤑😲😖😞😟😤😢😭😦😧😨😩🤯😬😰😱😳🤪😵😡😠😇😷🤓😎🤖🤗😻😼😽🙀😿😾 peko"
        );
        assert_eq!(
            pekofy_text("<:pekoFeet:828454981836996608><:pekoFeet:828454981836996608>").unwrap(),
            "<:pekoFeet:828454981836996608><:pekoFeet:828454981836996608> peko"
        );
        assert_eq!(
            pekofy_text("<:pekoFeet:828454981836996608>🐱").unwrap(),
            "<:pekoFeet:828454981836996608>🐱 peko"
        );
        assert_eq!(
            pekofy_text("🐱<:pekoFeet:828454981836996608>").unwrap(),
            "🐱<:pekoFeet:828454981836996608> peko"
        );
        assert_eq!(
            pekofy_text("<:pekoFeet:828454981836996608> <:pekoFeet:828454981836996608>").unwrap(),
            "<:pekoFeet:828454981836996608> <:pekoFeet:828454981836996608> peko"
        );
        assert_eq!(
            pekofy_text("<a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608>").unwrap(),
            "<a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608><a:AmongAss:841256576803012608> peko"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_multiple_emoji_with_peko() {
        assert_eq!(pekofy_text("🐱🐱 peko").unwrap(), "🐱🐱 peko");
        assert_eq!(pekofy_text("🐱 🐱 peko").unwrap(), "🐱 🐱 peko");
    }

    #[traced_test]
    #[test]
    fn pekofy_long_text() {
        assert_eq!(
            pekofy_text("My name is Yoshikage Kira. I’m 33 years old. My house is in the northeast section of Morioh, where all the villas are, and I am not married. I work as an employee for the Kame Yu department stores, and I get home every day by 8 PM at the latest. I don’t smoke, but I occasionally drink. I’m in bed by 11 PM, and make sure I get eight hours of sleep, no matter what. After having a glass of warm milk and doing about twenty minutes of stretches before going to bed, I usually have no problems sleeping until morning. Just like a baby, I wake up without any fatigue or stress in the morning.
I was told there were no issues at my last check-up. I’m trying to explain that I’m a person who wishes to live a very quiet life. I take care not to trouble myself with any enemies, like winning and losing, that would cause me to lose sleep at night. That is how I deal with society, and I know that is what brings me happiness. Although, if I were to fight I wouldn’t lose to anyone.").unwrap(),
            "My name is Yoshikage Kira peko. I’m 33 years old peko. My house is in the northeast section of Morioh, where all the villas are, and I am not married peko. I work as an employee for the Kame Yu department stores, and I get home every day by 8 PM at the latest peko. I don’t smoke, but I occasionally drink peko. I’m in bed by 11 PM, and make sure I get eight hours of sleep, no matter what peko. After having a glass of warm milk and doing about twenty minutes of stretches before going to bed, I usually have no problems sleeping until morning peko. Just like a baby, I wake up without any fatigue or stress in the morning peko.
I was told there were no issues at my last check-up peko. I’m trying to explain that I’m a person who wishes to live a very quiet life peko. I take care not to trouble myself with any enemies, like winning and losing, that would cause me to lose sleep at night peko. That is how I deal with society, and I know that is what brings me happiness peko. Although, if I were to fight I wouldn’t lose to anyone peko."
        );
        assert_eq!(
            pekofy_text("What is Lorem Ipsum?
Lorem Ipsum is simply dummy text of the printing and typesetting industry. Lorem Ipsum has been the industry's standard dummy text ever since the 1500s, when an unknown printer took a galley of type and scrambled it to make a type specimen book. It has survived not only five centuries, but also the leap into electronic typesetting, remaining essentially unchanged. It was popularised in the 1960s with the release of Letraset sheets containing Lorem Ipsum passages, and more recently with desktop publishing software like Aldus PageMaker including versions of Lorem Ipsum.

Why do we use it?
It is a long established fact that a reader will be distracted by the readable content of a page when looking at its layout. The point of using Lorem Ipsum is that it has a more-or-less normal distribution of letters, as opposed to using 'Content here, content here', making it look like readable English. Many desktop publishing packages and web page editors now use Lorem Ipsum as their default model text, and a search for 'lorem ipsum' will uncover many web sites still in their infancy. Various versions have evolved over the years, sometimes by accident, sometimes on purpose (injected humour and the like).").unwrap(),
            "What is Lorem Ipsum peko?
Lorem Ipsum is simply dummy text of the printing and typesetting industry peko. Lorem Ipsum has been the industry's standard dummy text ever since the 1500s, when an unknown printer took a galley of type and scrambled it to make a type specimen book peko. It has survived not only five centuries, but also the leap into electronic typesetting, remaining essentially unchanged peko. It was popularised in the 1960s with the release of Letraset sheets containing Lorem Ipsum passages, and more recently with desktop publishing software like Aldus PageMaker including versions of Lorem Ipsum peko.

Why do we use it peko?
It is a long established fact that a reader will be distracted by the readable content of a page when looking at its layout peko. The point of using Lorem Ipsum is that it has a more-or-less normal distribution of letters, as opposed to using 'Content here, content here', making it look like readable English peko. Many desktop publishing packages and web page editors now use Lorem Ipsum as their default model text, and a search for 'lorem ipsum' will uncover many web sites still in their infancy peko. Various versions have evolved over the years, sometimes by accident, sometimes on purpose (injected humour and the like peko)."
        );
    }

    #[traced_test]
    #[test]
    #[ignore]
    fn pekofy_arabic() {
        assert_eq!(
            pekofy_text("أنا بحاجة إلى أن أكون أحمر أثناء الأشهر الأولى").unwrap(),
            "أنا بحاجة إلى أن أكون أحمر أثناء الأشهر الأولى"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_numbers() {
        assert_eq!(
            pekofy_text("1 2 3 4 5 6 7 8 9 10").unwrap(),
            "1 2 3 4 5 6 7 8 9 10 peko"
        );

        assert_eq!(
            pekofy_text("3.14159265358979323846264338327950288419716939937510582097494459230781640628620899862803482534211706798214808651328230664709384460955058223172535940812848111745028410270193852110555964462294895493038196442881097566593344612847564823378678316527120190914564856692346034861045432664821339360726024914127372458700660631558817488152092096282925409171536436789259036001133053054882046652138414695194151160943305727036575959195309218611738193261179310511854807446237996274956735188575272489122793818301194912983367336244065664308602139494639522473719070217986094370277053921717629317675238467481846766940513200056812714526356082778577134275778960917363717872146844090122495343014654958537105079227968925892354201995611212902196086403441815981362977477130996051870721134999999837297804995105973173281609631859502445945534690830264252230825334468503526193118817101000313783875288658753320838142061717766914730359825349042875546873115956286388235378759375").unwrap(),
            "3.14159265358979323846264338327950288419716939937510582097494459230781640628620899862803482534211706798214808651328230664709384460955058223172535940812848111745028410270193852110555964462294895493038196442881097566593344612847564823378678316527120190914564856692346034861045432664821339360726024914127372458700660631558817488152092096282925409171536436789259036001133053054882046652138414695194151160943305727036575959195309218611738193261179310511854807446237996274956735188575272489122793818301194912983367336244065664308602139494639522473719070217986094370277053921717629317675238467481846766940513200056812714526356082778577134275778960917363717872146844090122495343014654958537105079227968925892354201995611212902196086403441815981362977477130996051870721134999999837297804995105973173281609631859502445945534690830264252230825334468503526193118817101000313783875288658753320838142061717766914730359825349042875546873115956286388235378759375 peko"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_punctuation() {
        assert_eq!(
            pekofy_text("!@#$%^&*()_+-=[]{};':,./<>?|").unwrap(),
            "!@#$%^&*()_+-=[]{};':,./<>?| peko"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_zalgo() {
        assert_eq!(
            pekofy_text(
                "3̶̍̌.̶̌̅1̸͗̊4̴́̈́1̵̾͝5̴̈́̀9̸͆͆2̴́̈́6̴́̄5̶̊̐3̶̇͂5̵͛̑8̸̄̔9̵̌̉7̵̾̽9̵̐̕3̴̉̈2̸̾̈́3̸̍̅8̶̓͊4̴̃̈6̴̉̅2̵̇͝6̴̛̽4̷̎͠3̶̂̑3̷͗͝8̷́̄3̸̐̌2̷̆̊7̷̑͠9̵̈́̎5̵̽́0̵̐̕2̴̇̋8̸̌̂8̴̓̽4̶̑́1̴̀͠9̷̉̈7̴̐̕1̶̛̈́6̷̀̈9̸͌̎3̷͊̉9̴̜͋9̷͑͝3̷͒̽7̴̿͘5̶͑105820974944592307816406286"
            )
            .unwrap(),
            "3̶̍̌.̶̌̅1̸͗̊4̴́̈́1̵̾͝5̴̈́̀9̸͆͆2̴́̈́6̴́̄5̶̊̐3̶̇͂5̵͛̑8̸̄̔9̵̌̉7̵̾̽9̵̐̕3̴̉̈2̸̾̈́3̸̍̅8̶̓͊4̴̃̈6̴̉̅2̵̇͝6̴̛̽4̷̎͠3̶̂̑3̷͗͝8̷́̄3̸̐̌2̷̆̊7̷̑͠9̵̈́̎5̵̽́0̵̐̕2̴̇̋8̸̌̂8̴̓̽4̶̑́1̴̀͠9̷̉̈7̴̐̕1̶̛̈́6̷̀̈9̸͌̎3̷͊̉9̴̜͋9̷͑͝3̷͒̽7̴̿͘5̶͑105820974944592307816406286 peko"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_emoticons() {
        assert_eq!(pekofy_text("ლ(ಠ益ಠლ)").unwrap(), "ლ(ಠ益ಠლ) peko");
        assert_eq!(
            pekofy_text("(╯°□°）╯︵ ┻━┻").unwrap(),
            "(╯°□°）╯︵ ┻━┻ peko"
        );
        assert_eq!(
            pekofy_text(r"(╯°□°）╯︵ ┻━┻（ ＾ν＾）┬─┬ノ( º ºノ)ಠಠ（・(ｪ)・）（・(ｪ)・）┻━┻ ︵ヽ(`Д´)ﾉ︵ ┻━┻༼ つ ◕◕ ༽つ(・o・)\ (•◡•) /ヽ(⌐■■)ノ♪♬\ (•◡•) /").unwrap(),
            r"(╯°□°）╯︵ ┻━┻（ ＾ν＾）┬─┬ノ( º ºノ)ಠಠ（・(ｪ)・）（・(ｪ)・）┻━┻ ︵ヽ(`Д´)ﾉ︵ ┻━┻༼ つ ◕◕ ༽つ(・o・)\ (•◡•) /ヽ(⌐■■)ノ♪♬\ (•◡•) / peko"
        );
    }

    #[traced_test]
    #[test]
    fn pekofy_latex() {
        assert_eq!(pekofy_text(r"\frac{1}{2}").unwrap(), r"\frac{1}{2} peko");
        assert_eq!(
            pekofy_text(
                r"$a^2+b^2=(a+b)^2$Yes, when $\text{char}(F)=2$. not quite, it's actually
$(x+2y)^2 = x^2 + 4xy + 4y^2$"
            )
            .unwrap(),
            "$a^2+b^2=(a+b)^2 peko$Yes, when $\text{char}(F)=2$ peko. not quite, it's actually
$(x+2y)^2 = x^2 + 4xy + 4y^2$ peko"
        );
    }
}
