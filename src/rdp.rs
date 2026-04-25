use crate::model::DistroRecord;

pub fn render_rdp_profile(distro: &DistroRecord, host: &str, port: u16) -> String {
    let username = distro.default_user.as_deref().unwrap_or_default();

    format!(
        "screen mode id:i:2\n\
use multimon:i:0\n\
span monitors:i:0\n\
desktopwidth:i:1920\n\
desktopheight:i:1080\n\
smart sizing:i:1\n\
dynamic resolution:i:1\n\
session bpp:i:32\n\
compression:i:1\n\
keyboardhook:i:2\n\
audiomode:i:2\n\
audiocapturemode:i:0\n\
redirectclipboard:i:1\n\
redirectprinters:i:0\n\
redirectsmartcards:i:0\n\
redirectdrives:i:0\n\
redirectcomports:i:0\n\
redirectposdevices:i:0\n\
autoreconnection enabled:i:1\n\
authentication level:i:2\n\
enablecredsspsupport:i:0\n\
connection type:i:7\n\
networkautodetect:i:0\n\
bandwidthautodetect:i:0\n\
disable wallpaper:i:1\n\
disable full window drag:i:1\n\
disable menu anims:i:1\n\
disable themes:i:1\n\
allow desktop composition:i:0\n\
allow font smoothing:i:0\n\
disable cursor setting:i:0\n\
bitmapcachepersistenable:i:1\n\
full address:s:{host}:{port}\n\
alternate full address:s:{host}:{port}\n\
prompt for credentials:i:1\n\
username:s:{username}\n"
    )
}

#[cfg(test)]
mod tests {
    use crate::model::DistroRecord;

    use super::render_rdp_profile;

    #[test]
    fn renders_profile_with_custom_port() {
        let profile = render_rdp_profile(
            &DistroRecord {
                default_user: Some("afsah".to_string()),
                ..DistroRecord::default()
            },
            "172.24.78.166",
            3390,
        );

        assert!(profile.contains("full address:s:172.24.78.166:3390"));
        assert!(profile.contains("alternate full address:s:172.24.78.166:3390"));
        assert!(profile.contains("enablecredsspsupport:i:0"));
        assert!(profile.contains("username:s:afsah"));
        assert!(profile.contains("connection type:i:7"));
        assert!(profile.contains("disable wallpaper:i:1"));
        assert!(profile.contains("use multimon:i:0"));
        assert!(profile.contains("audiomode:i:2"));
        assert!(profile.contains("dynamic resolution:i:1"));
        assert!(profile.contains("allow font smoothing:i:0"));
    }
}
