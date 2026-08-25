//! Wi-Fi (802.11) station model: scan records, security, channels.
//!
//! Pure data model — radio control belongs to the driver backend. The
//! station manager tracks visible networks and the current association.

use orbita_std::{String, Vec, format};

/// Wi-Fi security suites.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum WifiSecurity {
    Open,
    Wep,
    Wpa,
    Wpa2,
    Wpa3,
    Wpa2Wpa3,
}

impl WifiSecurity {
    pub fn label(self) -> &'static str {
        match self {
            WifiSecurity::Open => "open",
            WifiSecurity::Wep => "wep",
            WifiSecurity::Wpa => "wpa",
            WifiSecurity::Wpa2 => "wpa2",
            WifiSecurity::Wpa3 => "wpa3",
            WifiSecurity::Wpa2Wpa3 => "wpa2/wpa3",
        }
    }
}

/// One network seen in a scan.
#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: [u8; 6],
    /// 2.4/5 GHz channel number.
    pub channel: u8,
    /// Signal strength in dBm (typically -100..-30).
    pub rssi_dbm: i8,
    pub security: WifiSecurity,
}

impl WifiNetwork {
    /// Human band name from the channel number.
    pub fn band(&self) -> &'static str {
        if self.channel <= 14 {
            "2.4GHz"
        } else {
            "5GHz"
        }
    }

    /// One-line summary for logs.
    pub fn summary(&self) -> String {
        let [a, b, c, d, e, f] = self.bssid;
        format!(
            "{} bssid={a:02x}:{b:02x}:{c:02x}:{d:02x}:{e:02x}:{f:02x} ch={} {} rssi={}dBm {}",
            self.ssid,
            self.channel,
            self.band(),
            self.rssi_dbm,
            self.security.label()
        )
    }
}

/// Association state of the station.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum WifiLink {
    Disconnected,
    Associating,
    Connected { channel: u8 },
}

/// Wi-Fi station manager.
#[derive(Debug)]
pub struct WifiStation {
    pub link: WifiLink,
    pub networks: Vec<WifiNetwork>,
}

impl WifiStation {
    pub fn new() -> Self {
        Self {
            link: WifiLink::Disconnected,
            networks: Vec::new(),
        }
    }

    /// Replaces the scan results, sorted best-signal-first.
    pub fn set_scan_results(&mut self, mut networks: Vec<WifiNetwork>) {
        networks.sort_by(|a, b| b.rssi_dbm.cmp(&a.rssi_dbm));
        self.networks = networks;
    }

    /// Associates with the strongest network matching (a prefix of) the
    /// wanted SSID. Returns true on success.
    pub fn connect_strongest(&mut self, ssid_prefix: &str) -> bool {
        match self
            .networks
            .iter()
            .find(|n| n.ssid.as_str().starts_with(ssid_prefix))
        {
            Some(network) => {
                self.link = WifiLink::Connected {
                    channel: network.channel,
                };
                true
            }
            None => false,
        }
    }

    pub fn disconnect(&mut self) {
        self.link = WifiLink::Disconnected;
    }

    /// Summary line, e.g. `wifi: connected ch=6 visible=4`.
    pub fn summary(&self) -> String {
        match self.link {
            WifiLink::Disconnected => format!("wifi: disconnected visible={}", self.networks.len()),
            WifiLink::Associating => format!("wifi: associating visible={}", self.networks.len()),
            WifiLink::Connected { channel } => {
                format!("wifi: connected ch={channel} visible={}", self.networks.len())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use orbita_std::vec;

    fn net(ssid: &str, rssi: i8, channel: u8) -> WifiNetwork {
        WifiNetwork {
            ssid: String::from(ssid),
            bssid: [0xAA, 0xBB, 0xCC, 0, 0, 1],
            channel,
            rssi_dbm: rssi,
            security: WifiSecurity::Wpa2,
        }
    }

    #[test]
    fn scan_sorted_and_connect() {
        let mut station = WifiStation::new();
        station.set_scan_results(vec![net("weak", -80, 11), net("strong", -40, 6)]);
        assert_eq!(station.networks[0].ssid, "strong");
        assert!(station.connect_strongest("strong"));
        assert_eq!(station.link, WifiLink::Connected { channel: 6 });
        assert!(station.summary().contains("connected ch=6"));
    }

    #[test]
    fn connect_missing_ssid_fails() {
        let mut station = WifiStation::new();
        station.set_scan_results(vec![net("home", -50, 1)]);
        assert!(!station.connect_strongest("office"));
        assert_eq!(station.link, WifiLink::Disconnected);
    }
}
