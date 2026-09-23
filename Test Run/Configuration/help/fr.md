# Lare — Commandes

Tapez les commandes `/…` dans la ligne de commande de Lare Terminal (votre session PowerShell).
Le résultat de chaque commande slash apparaît dans une fenêtre ; une ligne de confirmation reste dans le terminal.

## IA
- `/ai "requête"` ou `/ "requête"` — l'IA répond et exécute des commandes **dans votre shell**
  (chaque commande proposée demande une confirmation `[Y/n]` avant de s'exécuter). Les guillemets sont obligatoires.

## Commandes
- `/help` — cette fenêtre.
- `/ping` — vérifie les couches de Lare (lare-shell, orchestrateur, plugin-ping, ui.exe).
- `/config` — configuration (apparence, recherche web, IA, marchés).
- `/library` — archive des documents enregistrés (réouvrables).
- `/aichat` — AI Chat (communication entre machines Lare en réseau, avec participation de l'IA).
- `/open <target>` — ouvre une URL, un dossier ou un fichier avec l'application par défaut. Pas
  de guillemets autour de la cible (contrairement à `/ai` ci-dessus) : tapez-la telle quelle,
  même avec des espaces.
- `/web <query>` — recherche la requête dans le navigateur par défaut.
- `/show <markdown>` — ouvre une fenêtre avec le Markdown indiqué.
- `/find [<query>] [in:"<expression>"] [folder:from-here]` — recherche de fichiers en direct, fenêtre dédiée.
- `/reset` — redémarre la session shell.
- `/nowin <requête>` — l'IA répond en texte dans le terminal, pas de fenêtre Markdown.
- `/calc` — calculatrice (plugin).

## Outils externes (fenêtre dédiée)
- `/markets` — outils pour les marchés financiers (recherche de ticker, rapport boursier, liste de titres, screener).
- `/netsec` — outils de scan réseau et de diagnostic (état du routeur, scan rapide, détection OS/version, découverte d'hôtes, recherche de vulnérabilités).
- `/pyping` — canal de test pour l'infrastructure des outils Python (écho de message).

## Tout le reste
- Toute ligne qui ne commence pas par `/` est du PowerShell, comme d'habitude.
- Une commande slash inconnue est ignorée silencieusement.
