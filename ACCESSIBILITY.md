# Accessibility

AppStack should be usable by everyone who uses Windows, including people who
rely on the keyboard, high contrast, screen magnification or a screen reader.
This statement describes what works today, what is known not to, and how to
tell us about barriers.

## Supported environments

- Windows 10 and Windows 11 (x64), with any display scaling. Every window
  follows the DPI of the monitor it is on.
- Built from native Win32 controls (windows, buttons, lists, menus, dialogs), so
  Windows' own accessibility features apply to them.

## What works

- **Keyboard:** the manager and editor can be used without a mouse. `F2` edits,
  `Delete` removes, `Enter` opens or launches and `Ctrl+N` creates a group. Menus
  have access keys. In popups, the arrow keys move between applications, `Enter`
  launches, `Page Up`/`Page Down` change page and `Esc` or `Backspace` go back
  one folder or close the popup.
- **High contrast:** when a Windows high contrast theme is on, it takes
  precedence over AppStack's themes and custom colors.
- **Text:** names cut short with an ellipsis show their full text in a tooltip.
  Title and label fonts can be changed in **Preferences**.
- **Motion:** popup animations are off by default and can be turned on or off in
  **Preferences > Popup animations**.

## Known limitations

- Screen readers have not been tested systematically. Native controls are
  announced, but popups draw their application tiles themselves, and some
  details (such as the page dots in scrolling popups) may not be announced.
- The **Popup animations** setting does not follow the Windows setting
  **Show animations in Windows**; turn it off in AppStack separately.
- The color picker preview and the live popup preview in the group editor are
  visual only.
- The manager's menu bar stays light in dark mode, because Windows does not theme
  it.

## Reporting a barrier

If something in AppStack is hard or impossible for you to use, please open an
issue with the **Bug report** form and start the title with
`Accessibility:`. Tell us what you were trying to do, what assistive technology
you use (for example Narrator, NVDA, JAWS, Magnifier or high contrast) and what
happened. Accessibility reports are treated as bugs, not feature requests.
