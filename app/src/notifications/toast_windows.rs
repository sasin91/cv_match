//! Windows' own notifications (toasts), raised as JobFinder: the app's name is registered under
//! the user's registry (HKCU\Software\Classes\AppUserModelId\JobFinder), which is how a program
//! that isn't installed from the Store names itself in a toast. `jobfinder cleanup` removes it.
//!
//! A toast clicked while this JobFinder runs gives its link to the window (Clicked); one raised
//! by the scheduled scan, whose process has ended, only closes when clicked.

use windows::Data::Xml::Dom::XmlDocument;
use windows::Foundation::TypedEventHandler;
use windows::UI::Notifications::{ToastActivatedEventArgs, ToastNotification, ToastNotificationManager};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR};
use windows::Win32::System::Registry::{HKEY_CURRENT_USER, REG_SZ, RegDeleteTreeW, RegSetKeyValueW};
use windows::core::{HSTRING, IInspectable, Interface, PCWSTR, Ref};

use super::Clicked;

/// The name toasts are raised under (the AppUserModelID), as Windows shows it in Settings >
/// Notifications too.
const APP_ID: &str = "JobFinder";
/// Where the name is registered, under HKEY_CURRENT_USER.
const APP_ID_KEY: &str = r"Software\Classes\AppUserModelId\JobFinder";
/// The name shown on the toast.
const DISPLAY_NAME: &str = "JobFinder";

/// Raises a toast saying title and text; clicked gets link when it is clicked while this process
/// runs.
pub fn show(title: &str, text: &str, link: &str, clicked: Option<Clicked>) -> Result<(), String> {
    register_name()?;
    let toast = toast(title, text, link).map_err(|e| format!("The notification couldn't be made: {e}"))?;
    if let Some(clicked) = clicked {
        let handler = TypedEventHandler::<ToastNotification, IInspectable>::new(
            move |_: Ref<ToastNotification>, args: Ref<IInspectable>| {
                if let Some(args) = args
                    .as_ref()
                    .and_then(|args| args.cast::<ToastActivatedEventArgs>().ok())
                {
                    clicked(args.Arguments()?.to_string());
                }
                Ok(())
            },
        );
        toast
            .Activated(&handler)
            .map_err(|e| format!("The notification's click couldn't be followed: {e}"))?;
    }
    ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(APP_ID))
        .and_then(|notifier| notifier.Show(&toast))
        .map_err(|e| format!("Windows didn't show the notification: {e}"))
}

/// The toast: title on its first line, text under it, link as what a click gives back.
fn toast(title: &str, text: &str, link: &str) -> windows::core::Result<ToastNotification> {
    let document = XmlDocument::new()?;
    document.LoadXml(&HSTRING::from(super::toast_xml(title, text, link)))?;
    ToastNotification::CreateToastNotification(&document)
}

/// Registers JobFinder's name for its toasts (written each time: it is two small values, and a
/// cleanup may have removed them meanwhile).
fn register_name() -> Result<(), String> {
    set_value("DisplayName", DISPLAY_NAME)
}

fn set_value(name: &str, value: &str) -> Result<(), String> {
    let key = HSTRING::from(APP_ID_KEY);
    let name = HSTRING::from(name);
    let data: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    // SAFETY: key and name are 0-ended UTF-16 that outlive the call; data is a 0-ended UTF-16
    // string of the byte length given.
    let set = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(key.as_ptr()),
            PCWSTR(name.as_ptr()),
            REG_SZ.0,
            Some(data.as_ptr().cast()),
            (data.len() * size_of::<u16>()) as u32,
        )
    };
    match set {
        ERROR_SUCCESS => Ok(()),
        e => Err(format!(
            "JobFinder's name for notifications couldn't be registered: {}",
            described(e)
        )),
    }
}

/// Removes JobFinder's name for toasts from the registry (cleanup); Ok(false) when it wasn't
/// there.
pub fn forget_name() -> Result<bool, String> {
    let key = HSTRING::from(APP_ID_KEY);
    // SAFETY: key is 0-ended UTF-16 that outlives the call.
    match unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr())) } {
        ERROR_SUCCESS => Ok(true),
        ERROR_FILE_NOT_FOUND => Ok(false),
        e => Err(described(e)),
    }
}

fn described(e: WIN32_ERROR) -> String {
    windows::core::Error::from(e.to_hresult()).message()
}
