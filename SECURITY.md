# Security policy

Drone Deck is an experimental preview. Security fixes target current main and the latest preview; older versions have no guaranteed maintenance window.

Do not publish credentials, exploitable vulnerabilities or sensitive vehicle data in a public issue. Use GitHub's **Report a vulnerability** option in this repository's Security tab when available. If unavailable, open a minimal issue asking for a private reporting channel, without vulnerability details or secrets.

Include the affected version, reproduction steps, impact and a minimal redacted example. Test only against computers, accounts and vehicles you own or have permission to test.

Hugging Face tokens belong in credential settings, never committed configuration. Review diagnostics before sharing; automatic redaction cannot recognize every sensitive value.

Drone Deck flight actions are limited to simulations it owns. A configured QGroundControl/scripts bridge can forward commands to a physical vehicle. Changes to that boundary, process launching, installation recipes and downloaded model handling require careful review.

There is no guaranteed response time or bug bounty at this stage.
