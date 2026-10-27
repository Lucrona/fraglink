//! Account email templates: plain text plus an HTML version (table layout, inline CSS,
//! no web fonts or external CSS) that holds up in Outlook, Hotmail/Outlook.com and Gmail.
//! The logo is a hosted PNG on the dark header colour, so it looks right with images on,
//! and the alt text reads "FRAGLINK" with images off.

pub struct Email {
    pub subject: String,
    pub text: String,
    pub html: String,
}

const LOGO: &str = "https://playfraglink.com/brand/email/fraglink-email-logo-302x64.png";
const FONT: &str = "Segoe UI, Helvetica, Arial, sans-serif";

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

struct Parts<'a> {
    preheader: &'a str,
    heading: &'a str,
    /// paragraphs, already HTML-escaped where needed
    paras: Vec<String>,
    button: Option<(&'a str, &'a str)>,
    after: Vec<String>,
    /// extra body HTML, inserted as is after the paragraphs (already escaped)
    raw: String,
    reason: &'a str,
}

fn layout(p: Parts) -> String {
    let para = |t: &str| {
        format!(
            r##"<p style="margin:0 0 16px 0;font-family:{FONT};font-size:15px;line-height:23px;color:#28313d;">{t}</p>"##
        )
    };
    let small = |t: &str| {
        format!(
            r##"<p style="margin:0 0 12px 0;font-family:{FONT};font-size:13px;line-height:20px;color:#5d6876;">{t}</p>"##
        )
    };
    let body: String = p.paras.iter().map(|t| para(t)).collect();
    let after: String = p.after.iter().map(|t| small(t)).collect();
    let button = match p.button {
        Some((label, url)) => {
            let (l, u) = (esc(label), esc(url));
            format!(
                r##"<table role="presentation" cellpadding="0" cellspacing="0" border="0" style="margin:6px 0 22px 0;"><tr>
<td align="center" bgcolor="#b8691f" style="border-radius:4px;background:#b8691f;">
<!--[if mso]><v:roundrect xmlns:v="urn:schemas-microsoft-com:vml" href="{u}" style="height:44px;v-text-anchor:middle;width:220px;" arcsize="10%" stroke="f" fillcolor="#b8691f"><center style="color:#ffffff;font-family:Segoe UI,Arial,sans-serif;font-size:15px;font-weight:bold;">{l}</center></v:roundrect><![endif]-->
<!--[if !mso]><!--><a href="{u}" target="_blank" style="display:inline-block;padding:12px 28px;font-family:{FONT};font-size:15px;font-weight:bold;line-height:20px;color:#ffffff;text-decoration:none;border-radius:4px;background:#b8691f;border:1px solid #9a5517;">{l}</a><!--<![endif]-->
</td></tr></table>
{fallback}"##,
                fallback = small(&format!(
                    r##"Button not working? Copy this link into your browser:<br><a href="{u}" style="color:#9a5517;word-break:break-all;">{u}</a>"##
                ))
            )
        }
        None => String::new(),
    };
    format!(
        r##"<!DOCTYPE html>
<html lang="en" xmlns="http://www.w3.org/1999/xhtml" xmlns:v="urn:schemas-microsoft-com:vml">
<head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1"><meta name="x-apple-disable-message-reformatting"><meta name="color-scheme" content="light only"><title>{title}</title></head>
<body style="margin:0;padding:0;background:#e9edf2;">
<div style="display:none;max-height:0;overflow:hidden;mso-hide:all;font-size:1px;line-height:1px;color:#e9edf2;">{pre}</div>
<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" bgcolor="#e9edf2" style="background:#e9edf2;"><tr><td align="center" style="padding:28px 12px;">
<table role="presentation" width="560" cellpadding="0" cellspacing="0" border="0" style="width:100%;max-width:560px;">
<tr><td bgcolor="#10161f" style="background:#10161f;padding:20px 28px;border-radius:6px 6px 0 0;border-bottom:3px solid #b8691f;">
<a href="https://playfraglink.com" target="_blank" style="text-decoration:none;"><img src="{LOGO}" width="151" height="32" alt="FRAGLINK" style="display:block;border:0;outline:none;color:#ffffff;font-family:{FONT};font-size:20px;font-weight:bold;letter-spacing:3px;"></a>
</td></tr>
<tr><td bgcolor="#ffffff" style="background:#ffffff;padding:30px 28px 14px 28px;">
<h1 style="margin:0 0 18px 0;font-family:{FONT};font-size:22px;line-height:28px;font-weight:bold;color:#10161f;">{title}</h1>
{body}{raw}{button}{after}
</td></tr>
<tr><td bgcolor="#f4f6f9" style="background:#f4f6f9;padding:16px 28px;border-radius:0 0 6px 6px;border-top:1px solid #e1e6ec;">
<p style="margin:0 0 6px 0;font-family:{FONT};font-size:12px;line-height:18px;color:#7a8592;">{reason}</p>
<p style="margin:0;font-family:{FONT};font-size:12px;line-height:18px;color:#7a8592;">Questions? <a href="mailto:support@playfraglink.com" style="color:#7a8592;">support@playfraglink.com</a> &middot; <a href="https://playfraglink.com" style="color:#7a8592;">playfraglink.com</a></p>
</td></tr>
</table></td></tr></table>
</body></html>"##,
        title = esc(p.heading),
        pre = esc(p.preheader),
        raw = p.raw,
        reason = esc(p.reason),
    )
}

/// Staff alert (Admin > Site settings > Email alerts): a heading, an intro line, a list of
/// items (plain text, escaped here) shown as a striped table, and a button into Admin.
pub fn alert(
    subject: &str,
    heading: &str,
    intro: &str,
    lines: &[String],
    button: (&str, &str),
) -> Email {
    let rows: String = lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            format!(
                r##"<tr><td style="padding:8px 10px;font-family:{FONT};font-size:14px;line-height:20px;color:#28313d;border-top:1px solid #e1e6ec;background:{bg};">{}</td></tr>"##,
                esc(l),
                bg = if i % 2 == 0 { "#ffffff" } else { "#f7f9fb" }
            )
        })
        .collect();
    let table = if lines.is_empty() {
        String::new()
    } else {
        format!(
            r##"<table role="presentation" width="100%" cellpadding="0" cellspacing="0" border="0" style="border:1px solid #e1e6ec;border-top:0;border-radius:4px;margin:0 0 6px 0;">{rows}</table>"##
        )
    };
    let mut text = format!("{heading}\n\n{intro}\n\n");
    for l in lines {
        text.push_str(&format!("- {l}\n"));
    }
    text.push_str(&format!("\n{}: {}\n\nFraglink", button.0, button.1));
    let mut paras = vec![esc(intro)];
    if !table.is_empty() {
        paras.push(table);
    }
    Email {
        subject: subject.to_string(),
        text,
        html: layout(Parts {
            preheader: intro,
            heading,
            paras,
            button: Some(button),
            after: vec![],
            raw: String::new(),
            reason: "You got this because admin email alerts are on in Admin > Site settings.",
        }),
    }
}

pub fn confirm(name: &str, url: &str) -> Email {
    Email {
        subject: "Confirm your email for Fraglink".into(),
        text: format!(
            "Hi {name},\n\nConfirm this email address for your Fraglink account:\n{url}\n\nThe link works for 3 days. If you didn't sign up, ignore this email.\n\nFraglink"
        ),
        html: layout(Parts {
            preheader: "One click to confirm your email address. The link works for 3 days.",
            heading: "Confirm your email",
            paras: vec![
                format!("Hi {},", esc(name)),
                "Please confirm this email address for your Fraglink account. We use it for password resets and sign-in alerts.".into(),
            ],
            button: Some(("Confirm email", url)),
            after: vec!["The link works for 3 days. If you didn't sign up for Fraglink, you can ignore this email.".into()],
            raw: String::new(),
            reason: "You got this because this address was used to sign up for Fraglink, or added to an account.",
        }),
    }
}

pub fn reset(name: &str, url: &str) -> Email {
    Email {
        subject: "Reset your Fraglink password".into(),
        text: format!(
            "Hi {name},\n\nReset your Fraglink password here (works for 1 hour):\n{url}\n\nIf you didn't ask for this, ignore this email.\n\nFraglink"
        ),
        html: layout(Parts {
            preheader: "Your password reset link. It works for 1 hour.",
            heading: "Reset your password",
            paras: vec![
                format!("Hi {},", esc(name)),
                "Someone (hopefully you) asked to reset the password for your Fraglink account. Choose a new one with the button below.".into(),
            ],
            button: Some(("Reset password", url)),
            after: vec!["The link works for 1 hour and only once. If you didn't ask for this, ignore this email; your password stays the same.".into()],
            raw: String::new(),
            reason: "You got this because a password reset was requested for your Fraglink account.",
        }),
    }
}

pub fn signin(name: &str, what: &str, security_url: &str) -> Email {
    Email {
        subject: format!("New sign-in from {what}"),
        text: format!(
            "Hi {name},\n\nYour Fraglink account was just signed in from {what}.\n\nIf this was you, there's nothing to do. If not, change your password and sign out other devices at {security_url}.\n\nFraglink"
        ),
        html: layout(Parts {
            preheader: "Your Fraglink account was just signed in. Not you? Secure your account.",
            heading: "New sign-in to your account",
            paras: vec![
                format!("Hi {},", esc(name)),
                format!(
                    r##"Your Fraglink account was just signed in from <b style="color:#10161f;">{}</b>."##,
                    esc(what)
                ),
                "If this was you, there's nothing to do.".into(),
            ],
            button: Some(("Review security", security_url)),
            after: vec!["Not you? Change your password and sign out other devices from the Security tab.".into()],
            raw: String::new(),
            reason: "You got this because sign-in alerts are on for your Fraglink account.",
        }),
    }
}

pub fn friend_request(
    to_name: &str,
    from_name: &str,
    from_handle: &str,
    friends_url: &str,
    turn_off_url: &str,
) -> Email {
    let who = if from_name.is_empty() || from_name == from_handle {
        format!("@{from_handle}")
    } else {
        format!("{from_name} (@{from_handle})")
    };
    let pre = format!("{who} sent you a friend request.");
    let text_body = format!(
        "Hi {to_name},\n\n{who} sent you a friend request on Fraglink.\n\nSee it here:\n{friends_url}\n\nTurn off these emails:\n{turn_off_url}\n\nFraglink"
    );
    Email {
        subject: format!("{who} wants to be friends on Fraglink"),
        text: text_body,
        html: layout(Parts {
            preheader: &pre,
            heading: "New friend request",
            paras: vec![
                format!("Hi {},", esc(to_name)),
                format!(
                    r##"<b style="color:#10161f;">{}</b> wants to be friends on Fraglink."##,
                    esc(&who)
                ),
            ],
            button: Some(("See friend request", friends_url)),
            after: vec![format!(
                r##"Don't want these emails? <a href="{}" style="color:#5b6b7c;">Turn off friend request emails</a>."##,
                esc(turn_off_url)
            )],
            raw: String::new(),
            reason: "You got this because friend request emails are on for your Fraglink account.",
        }),
    }
}

/// Welcome email, sent once after a new account confirms its email.
pub fn welcome(nickname: &str, username: &str) -> Email {
    let body = include_str!("email_templates/welcome.html")
        .replace("{nickname}", &esc(nickname))
        .replace("{username}", &esc(username));
    let text = include_str!("email_templates/welcome.txt")
        .replace("{nickname}", nickname)
        .replace("{username}", username);
    Email {
        subject: "Welcome to Fraglink".into(),
        text,
        html: layout(Parts {
            preheader: "Get the app, bring your Steam and WoW hours, and add your friends.",
            heading: "Welcome to Fraglink",
            paras: vec![],
            button: None,
            after: vec![],
            raw: body,
            reason: "You got this once because you just created a Fraglink account.",
        }),
    }
}

#[cfg(test)]
mod tests {
    /// `EMAIL_PREVIEW_DIR=/tmp/x cargo test -p fraglink-server --lib emails -- --ignored`
    #[test]
    #[ignore]
    fn write_previews() {
        let dir = std::env::var("EMAIL_PREVIEW_DIR").unwrap();
        let site = "https://playfraglink.com";
        for (n, e) in [
            (
                "confirm",
                super::confirm(
                    "Lucrona",
                    &format!("{site}/verify?token=EXAMPLE-TOKEN-1234567890abcdef"),
                ),
            ),
            (
                "reset",
                super::reset(
                    "Lucrona",
                    &format!("{site}/reset?token=EXAMPLE-TOKEN-1234567890abcdef"),
                ),
            ),
            (
                "signin",
                super::signin(
                    "Lucrona",
                    "Chrome on Windows, Sydney, Australia",
                    &format!("{site}/settings#security"),
                ),
            ),
            (
                "friend_request",
                super::friend_request(
                    "Lucrona",
                    "Riftwalker",
                    "riftwalker",
                    &format!("{site}/friends"),
                    &format!("{site}/api/v1/notify/off?token=EXAMPLE"),
                ),
            ),
            ("welcome", super::welcome("Lucrona", "lucrona")),
        ] {
            std::fs::write(format!("{dir}/email-{n}.html"), &e.html).unwrap();
            std::fs::write(
                format!("{dir}/email-{n}.txt"),
                format!("Subject: {}\n\n{}", e.subject, e.text),
            )
            .unwrap();
        }
    }

    #[test]
    fn templates_escape_names_and_carry_links() {
        let e = super::confirm(
            "<b>Jo & Co</b>",
            "https://playfraglink.com/verify?token=abc",
        );
        assert!(e.html.contains("&lt;b&gt;Jo &amp; Co&lt;/b&gt;"));
        assert!(!e.html.contains("<b>Jo"));
        assert!(e.html.contains("https://playfraglink.com/verify?token=abc"));
        assert!(e.text.contains("https://playfraglink.com/verify?token=abc"));
        let s = super::signin(
            "Lu",
            "Windows, Sydney",
            "https://playfraglink.com/settings#security",
        );
        assert_eq!(s.subject, "New sign-in from Windows, Sydney");
        assert!(super::reset("Lu", "https://x/reset?token=1")
            .html
            .contains("Reset password"));
        let fr = super::friend_request(
            "Lu",
            "Bob <x>",
            "bob",
            "https://playfraglink.com/friends",
            "https://playfraglink.com/api/v1/notify/off?token=abc",
        );
        assert!(fr.subject.contains("@bob"));
        assert!(fr.html.contains("Bob &lt;x&gt;"));
        assert!(fr.html.contains("Turn off friend request emails"));
        assert!(fr.text.contains("https://playfraglink.com/friends"));
        let w = super::welcome("<i>Lu</i>", "lu&co");
        assert_eq!(w.subject, "Welcome to Fraglink");
        assert!(w.html.contains("Hi &lt;i&gt;Lu&lt;/i&gt;,"));
        assert!(w.html.contains("@lu&amp;co"));
        assert!(!w.html.contains("{nickname}") && !w.text.contains("{username}"));
        assert!(w.text.starts_with("Hi <i>Lu</i>,"));
        assert!(!w.text.contains("Subject:"));
        assert!(w
            .html
            .contains("https://www.curseforge.com/wow/addons/fraglink-played"));
    }
}
