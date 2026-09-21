# Alpine Rush — démo Void

Mini-jeu de course arcade en minecart, **solo ou jusqu'à huit joueurs**, pour
**Minecraft Java 26.1.2**, sans mod ni resource pack. Le véhicule se pilote librement
sur une chaussée : ses déplacements sont simulés par le serveur.

> **État du portage.** Cette version couvre le socle (D0) du portage de la démo sur les
> API actuelles du moteur — génération du monde, géométrie du circuit et simulation
> pure des karts —, la machine à états de la course (D1) : phases, commandes,
> annonces et bossbars, les karts en tant qu'entités du moteur (D2) : minecart
> monté, pilotage au clavier et déplacement piloté par le serveur, l'arsenal
> arcade (D3) : cristaux de bonus, pièges, missiles, ondes et particules, les
> affichages animés (D4) : bonus tenu, réacteurs, bouclier, recharge, débuffs, pièges,
> missiles et ondes en entités *display* persistantes, et les déplacements (D5) :
> vol libre des spectateurs, barrière de téléportation et embarquement dans le kart
> une fois les chunks de la grille reçus par le client, puis la passe d'interface (D6) :
> chat hiérarchisé et coloré, barre d'action pour l'éphémère, sons, seed aléatoire et
> particules d'impact.

## Lancer

Depuis la racine du dépôt :

```sh
cargo run --release -p voidmc-demo
```

La démo écoute par défaut sur toutes les interfaces IPv4 (`0.0.0.0:25565`).
Se connecter à `127.0.0.1:25565` depuis la machine hôte, ou à son adresse IP depuis
une autre machine. Le joueur apparaît à **Y = 110**, au-dessus de la vallée, en
**vol libre** (vol autorisé, invulnérable, vitesse de vol 0,07) ; son minecart l'attend,
invisible, sur la ligne de stationnement aérienne. Aucune plateforme ne gêne la vue du
paysage.

Sans `VOID_DEMO_SEED`, la carte est **tirée au hasard** à chaque lancement ; la seed
choisie est écrite dans le log de démarrage (`seed aleatoire : VOID_DEMO_SEED=… pour
rejouer cette carte`). Pour rejouer une carte précise :

```sh
VOID_DEMO_SEED=2026 cargo run --release -p voidmc-demo
```

`VOID_DEMO_ADDRESS` permet de modifier l'adresse et le port d'écoute. La seed est un
entier non signé sur 64 bits (jamais zéro quand elle est tirée au hasard). Le serveur
tourne à 20 ticks/s. `RUST_LOG` règle la verbosité des logs.

## Jouer

Chaque pilote inscrit possède **un minecart** (`src/vehicle.rs`). Le minecart est une
entité du moteur : le serveur le fait apparaître chez les joueurs qui ont chargé son
chunk, le retire quand ils s'éloignent ou se déconnectent, et transmet ses déplacements.
Entre deux manches il est **stationné, vide et caché** (`voidmc::Hidden`) sur la ligne
d'attente à Y = 110 : aucun kart vide ne flotte au-dessus de la vallée pendant que le
pilote vole librement. Le marqueur est retiré à l'embarquement, si bien que le kart
apparaît (`SpawnEntity`) sur la grille avant la barrière et `SetPassengers` ; il est
remis dès que le pilote repart en vol (fin de manche, `/leave`, délai dépassé, annulation).
Il ne devient le passager qu'à l'embarquement sur la grille, une fois la barrière de
téléportation franchie (`src/travel.rs`). Sur une pression de Sneak,
le serveur renvoie la liste des passagers du kart (fidèle à la référence) — uniquement
sur cette pression, jamais périodiquement ; comme la descente d'un véhicule est
décidée côté serveur en 26.1.2 et que ce moteur ne l'implémente pas, le client ne
quitte jamais son kart de lui-même.

Le pilotage utilise les touches de déplacement du client (`ServerboundPlayerInput`) :
**Avancer** accélère, **Reculer** freine puis passe en marche arrière, **Gauche/Droite**
tournent, **Saut** enclenche le boost (tant qu'il reste du carburant), **Sprint**
utilise le bonus ramassé. Les règles de conduite (accélération, traînée,
boost rechargeable, rebonds sur les glissières, chocs entre minecarts) sont celles de
`src/kart.rs` ; elles ne s'appliquent que pendant la course. À chaque tick, le
serveur déplace l'entité minecart vers la position simulée ; le moteur choisit
lui-même entre un déplacement relatif et une téléportation (au-delà de 8 blocs, par
exemple lors de la mise en grille). Un kart immobile ne génère aucun paquet.

Un client assis dans un minecart ne rapporte plus sa position (il n'envoie que sa
rotation) : `pose` écrit donc aussi la `Position` du pilote, 0,35 bloc au-dessus du
kart comme dans la référence, pour que le chargement des chunks et la position vue
par les autres joueurs suivent le kart pendant toute la course.

La manche est complète (`src/race.rs`) :

- `/race [tours] [seed]` accepte **1 à 20 tours** (3 par défaut) et, en option, la
  **seed d'un circuit** (entier non signé sur 64 bits) pour le **rejouer** à
  l'identique. Sans seed, chaque manche tire un nouveau circuit de la séquence
  déterminée par `VOID_DEMO_SEED` ; un rejeu consomme lui aussi un numéro de
  manche, donc la manche suivante sans seed tire un circuit différent de celui
  qu'elle aurait tiré sans le rejeu. La seed du circuit est écrite dans le log au
  lancement et affichée dans le chat au lancement (« Circuit #… ») comme à la fin
  de la manche, avec la commande exacte pour le rejouer. Le nombre de tours choisi
  vaut pour tous les pilotes de la manche ; une commande invalide ne lance pas de
  course, et une manche en cours ne peut pas être relancée.
- `/race` renvoie d'abord **tout le monde en vol au-dessus du point d'attente**
  (`Teleport` du moteur vers `travel::LOBBY`, orientation 180°/15°) et décroche les
  pilotes de leur kart. Chaque manche construit ensuite un **nouveau tracé** dans la
  vallée, un chunk tous les deux ticks ; la progression est affichée par le panneau
  latéral et le HUD.
- Circuit prêt : les karts des participants sont placés sur la grille (téléportation
  d'entité par le moteur) et chaque pilote **embarque** via la barrière de
  téléportation du moteur (`Teleport::to(siège).facing(cap, 0)`) : position tenue par
  le serveur (`ServerControlledPosition`) pendant tout le transfert, chunks de la
  destination envoyés à raison de **deux par tick** (`ChunkSendBudget`), *Ping/Pong*
  pour s'assurer que le client les a traités, synchronisation de position, puis
  confirmation du client. Le kart est donc toujours apparu chez le pilote **avant**
  `SetPassengers`. À la confirmation (`PlayerTeleportEvent::Confirmed`), le pilote
  est installé dans le kart, garde une position pilotée par le serveur et perd le
  vol. La phase de chargement dure tant qu'un participant n'a pas confirmé ; un
  client qui ne répond pas en **30 secondes** (`TimedOut`) est retiré de la manche,
  prévenu, et renvoyé en vol, son kart disparaissant de la grille ; un transfert
  annulé (`Cancelled`) retire lui aussi le pilote de la manche (son kart est caché, il
  reste en vol) et une déconnexion libère simplement le joueur. Le compte à rebours de
  cinq secondes démarre quand tous les participants sont à bord.
- Le nombre de tours choisi, avec huit checkpoints par tour, à franchir dans l'ordre.
  Chaque nouveau tour est signalé au pilote concerné sur sa barre d'action ; le chat
  annonce les arrivées, les records du circuit et le podium.
  La limite de temps est de dix minutes jusqu'à trois tours, puis de 200 secondes par
  tour ; le chat prévient à 60, 30 et 10 secondes de la fin.
- La **barre d'action** des pilotes affiche la phase, puis tour, checkpoint, temps,
  pénalités, vitesse et bonus pendant la course. La **bossbar bleue** de chaque pilote
  affiche sa réserve de boost ; une bossbar partagée suit le compte à rebours et le
  temps restant. Le **panneau latéral** (ci-dessous) porte l'état persistant : rang
  en direct, tour, chrono, meilleur tour et record.
- `/reset` ramène au dernier checkpoint avec trois secondes de pénalité ; le minecart
  y est téléporté par le moteur.
- `/scores` affiche les temps de la manche et les meilleurs temps du circuit actuel
  pour le même nombre de tours. Les records sont en mémoire, par seed de circuit,
  nombre de tours et nom de joueur, et disparaissent au redémarrage. Un pilote qui
  bat le meilleur temps connu du circuit (tous pilotes confondus) déclenche une
  annonce **Record du circuit** dans le chat : elle n'a de sens que sur un circuit
  rejoué (`/race [tours] <seed>`), puisqu'un circuit tiré au hasard n'est jamais
  couru deux fois.
- `/leave` passe en spectateur : le kart disparaît et le joueur repart **en vol** vers
  le point d'attente par la même barrière ; `/join` inscrit pour le prochain départ
  sans renvoyer d'abilities déjà en place. Chaque joueur qui arrive est inscrit
  automatiquement (huit pilotes au maximum) et reçoit le vol (`PlayerAbilities` du
  moteur, drapeaux 0x07, vitesses 0,07 / 0,1) ; s'il coupe le vol en l'air, le serveur
  le lui rend aussitôt. Une arrivée ou un retour pendant une manche attend la suivante,
  en vol ; un pilote qui se déconnecte pendant la course abandonne la manche.
- Fin de manche : tout le monde est renvoyé en vol au point d'attente, les karts se
  garent, cachés, puis le circuit se démonte.
- Lorsque tous les participants ont terminé ou quitté, le circuit se démonte dans
  l'ordre inverse et le terrain initial est restauré ; `/race` redevient disponible
  à la fin du démontage.

### Interface & sons

Le chat ne reçoit que des **événements** : arrivées et départs de joueurs, lancement
de manche, GO, arrivées, records, podium, fin de manche, avertissements de temps,
délais dépassés et erreurs de commande. Tout ce qui est éphémère — tour bouclé,
bonus ramassé ou activé, coup reçu, bouclier, retour au checkpoint, missile sans
cible — passe par la **barre d'action** (`Chat::flash`) ; la progression de
construction, « tous les pilotes chargés » et le compte à rebours chiffré ont disparu
du chat, le panneau latéral, la bossbar et le HUD les affichent déjà. Aucun préfixe : la couleur porte la
catégorie, définie une seule fois dans `src/chat.rs` (`Tone`) :

| Ton | Couleur | Usage |
|---|---|---|
| `Info` | gris | système : arrivées/départs, aide, démontage, `/scores` |
| `Event` | or | événements de course : lancement, GO, arrivée, fin de manche |
| `Notice` | aqua | notices personnelles : statut à l'arrivée, spectateur, bonus, tour |
| `Good` | vert | bonus activé, bouclier qui absorbe |
| `Warn` | rouge | erreurs de commande, temps, délai, coup reçu, `/reset` |
| `Alert` | jaune | dernier tour |
| `Record` | violet clair | records du circuit |
| podium | or / gris / bronze | 1er / 2e / 3e |

Un flash occupe la barre d'action pendant **huit rafraîchissements du HUD** (2 s,
`chat::FLASH_PERIODS`) : le HUD, émis toutes les cinq ticks en or, ne réécrit la barre
qu'une fois le flash expiré (`Chat::hud_free`, composant `Flash` sur le pilote).
**Un flash par tick, le dernier gagne** : deux flashs adressés au même joueur au même
tick (bonus ramassé et missile reçu, par exemple) ne sont pas mis en file, seul le
dernier paquet reste à l'écran.

Les **sons** passent par l'API `Sounds` du moteur (`src/audio.rs`, `Cue`) ; chaque
identifiant est résolu dans le registre `sound_event` de `voidmc_data`, et un test
vérifie qu'ils existent tous. Les sons d'interface (`play_to`, catégorie *UI*) vont au
seul joueur concerné ; les sons de jeu sont émis **depuis l'entité kart**
(`EntitySoundEffect`, catégorie *Players*) pour les joueurs qui voient son chunk :

| Événement | Son | Cible |
|---|---|---|
| Compte à rebours 3-2-1 / GO | `block.note_block.pling` grave (1.0) / aigu (2.0) | tous |
| Départ | `entity.firework_rocket.large_blast` | tous |
| Tour bouclé / dernier tour | `block.note_block.bell` (1.5) / `block.bell.use` | pilote |
| Arrivée / record | `entity.player.levelup` / `ui.toast.challenge_complete` | pilote / tous |
| Bonus ramassé | `entity.item.pickup` (1.2) | pilote |
| Turbo / Bouclier / Banane / Glace | `firework_rocket.launch` / `beacon.activate` / `slime_block.place` / `glass.place` | au kart |
| Missile / Onde / Éclair / Super-recharge | `wither.shoot` / `generic.explode` / `lightning_bolt.thunder` / `respawn_anchor.charge` | au kart |
| Touché : missile / éclair / onde / banane / glace | `generic.explode` / `lightning_bolt.impact` / `player.hurt` / `slime.squish` / `glass.break` | au kart |
| Bouclier qui absorbe | `item.shield.block` | au kart |
| Choc (kart ou glissière) | `block.anvil.land` (0.5, 1.6) | au kart |
| Retour au point d'attente, délai dépassé | `block.portal.trigger` (0.6), placé au point d'attente | joueur |

Les chocs sont limités à **un son par kart toutes les dix ticks** (`Kart::knock`,
`kart::BUMP_COOLDOWN`), quels que soient les rebonds ou contacts intermédiaires ; le
choc à sonner est un drapeau `Kart::knocked` consommé par `vehicle::bumps`, si bien
qu'un choc sur la ligne d'arrivée ne sonne qu'une fois ; un kart protégé qui reste sur
une nappe de glace ne produit ni son ni flash.

Les **impacts** ajoutent une rafale de particules **une seule fois, au tick de
l'événement** (`Items::sparks`, vidé par `effects`), destinataires résolus une fois par
rafale : explosion + grosse fumée + flammes + crit pour un missile, une onde de choc ou
un coup d'onde ; explosion + étincelles pour un éclair ; flocons et boules de neige
étalés dans l'axe du kart pour la glace, teinture jaune et nuages pour la banane ;
un anneau de douze flammes (rayon 1,2) à l'activation du turbo ; une coque de trente
étincelles quand le bouclier absorbe. Les traînées, orbites et anneaux gardent leur
cadence.

### Panneau latéral

Chaque joueur connecté — pilote ou spectateur — a **son propre panneau** à droite de
l'écran (`src/sidebar.rs`, un composant `Sidebar` du moteur sur l'entité joueur,
audience `viewers([joueur])`), titré « ✦ Alpine Rush ✦ » en or. La liste d'onglets
(`TabList`, entité unique) porte l'en-tête « Alpine Rush » et le pied
« Circuit #seed · /race [tours] [seed] », réécrit quand la seed change. Le contenu
suit la phase, en trois dispositions (`sidebar::Layout`) ; changer de disposition
reconstruit le panneau une seule fois :

| Phase | Disposition | Lignes |
|---|---|---|
| Lobby, Results | `Idle` | `Circuit: #seed`, `Manche: n`, les derniers résultats (top 5, temps en `m:ss.d`, or / gris pour les deux premiers, jaune pour soi) ou « Aucun resultat : /race », `Record` |
| Generating, Loading, Destroying | `Building` | `Circuit`, `Manche`, une jauge `Construction` / `Chargement` / `Demontage` sur dix cases et `Avancement: n%`, `Record` |
| Countdown, Racing | `Racing` | `Circuit`, `Tour: x/N` (ou `Spectateur`, `Arrivee`), `Position: r/n`, `Temps: mm:ss`, le **classement** (top 5, `1. Nom T2` ou `1. Nom 1:23.4` une fois arrivé, soi en jaune, arrivés en vert), `Meilleur tour`, `Record` |

Le classement est **le même ordre que celui des missiles** : pilotes arrivés d'abord
(par temps), puis les autres par `Kart::progress` (tours + fraction du tour, avancée
vers la porte suivante comprise). Il est calculé une fois par tick pour tout le monde
(ressource `Standings`, huit karts au plus) ; il ne porte que des identités et des
tours, pas de distances, si bien que sa **version** ne change que lorsque l'ordre, un
tour ou une arrivée change — deux karts qui se dépassent produisent une seule
réécriture chez chacun, deux karts qui roulent sans se dépasser n'en produisent aucune.

**Poussé sur changement, strictement.** Le panneau ne compare que des valeurs
quantifiées (`View` : disposition, seed, manche, tour, position, secondes du chrono,
pourcentage, meilleur tour, record, version du classement) avec ce qu'il a déjà
affiché ; rien n'est formaté ni écrit tant qu'elles sont égales, et seul le widget
dont la valeur a changé est réécrit. Le moteur n'envoie ensuite que les lignes dont le
texte diffère : en régime établi un pilote reçoit **une ligne par seconde** (le chrono),
plus une réécriture immédiate à chaque changement de tour, de rang ou de meilleur
tour. Un tick sans changement n'émet aucun paquet.

Le **meilleur tour** est chronométré par `Racer` (`lap_start`, `best_lap`) : à chaque
franchissement de la ligne (`racing`, sur le même événement de porte que les messages
de tour) le temps du tour est `tick − max(lap_start, départ)` et le meilleur est
conservé jusqu'à la manche suivante, qui l'efface à la mise en grille. Le **record**
affiché est le meilleur temps connu du circuit pour ce nombre de tours, y compris la
manche en cours (`min(race.record, results[0])`) ; l'annonce de record dans le chat
reste jugée contre le meilleur temps d'avant la manche.

Sans doublon avec la bossbar : la construction et le démontage n'y sont plus (le panneau
les porte), la bossbar partagée ne garde que l'éphémère — compte à rebours et temps
restant — et la barre d'action reste le HUD instantané.

### Power-ups et effets

Dès que le circuit est prêt, **24 End Crystals** flottent au-dessus de la piste
(`src/items.rs`) : trois par checkpoint, à gauche, au centre et à droite, à mi-chemin
du checkpoint suivant. Chaque cristal est une entité du moteur (`EntityBuilder` +
`EndCrystal::floating()`, sans socle ni rayon) que le serveur fait apparaître chez les
joueurs qui ont chargé son chunk. Un pilote qui passe à moins de 1,6 bloc d'un
cristal, sans bonus en main, ramasse un des huit bonus (tirage déterministe à partir de
la seed du circuit, du tick, de l'emplacement et du kart) ; le cristal disparaît et
réapparaît **huit secondes** plus tard. Un kart stationné sur l'emplacement ramasse le
bonus au tick même de la réapparition, sans que le cristal ne réapparaisse ; deux karts
sur le même cristal au même tick ne donnent qu'un bonus, au plus petit identifiant
d'entité. Quatre particules `end_rod` orbitent autour de chaque cristal présent, toutes
les dix ticks, pour ses seuls spectateurs.

**Sprint** active le bonus tenu. Les effets sur le véhicule sont ceux de `src/kart.rs`
(`Kart::activate`, `Kart::strike`) ; la partie « monde » vit dans `src/items.rs` :

- **Turbo**, **Bouclier**, **Super-recharge** : effets sur le kart seul ; la recharge
  émet une onde `happy_villager` de 3 blocs.
- **Banane** et **Nappe de glace** : un piège déposé 2,8 blocs derrière le kart, actif
  12 s (banane, 1,5 bloc, consommée au contact) ou 10 s (glace, 2,5 blocs,
  persistante). Le poseur n'est pas concerné pendant une seconde. Une banane fait
  déraper, la glace réduit l'adhérence 3 s ; le bouclier absorbe les deux.
- **Missile guidé** : vise le pilote le plus proche devant le tireur (ordre de
  progression) et remonte la piste à 2 blocs/tick en glissant latéralement vers sa
  cible (0,3 bloc/tick, ±5,5 blocs) ; il touche à 2,5 blocs, s'éteint après 8 s ou si
  la cible quitte la course. Sans cible, il file le long de la piste.
- **Onde de choc** : repousse et fait tourner tous les pilotes à moins de 9 blocs.
  **Éclair** : ralentit tous les adversaires en course 2,5 s, où qu'ils soient.

Chaque impact, onde ou éclair produit une **onde** (`Burst`) qui vieillit 12 ticks et
dont le rayon grandit en `ease_out` ; elle est dessinée tous les quatre ticks par un
anneau de six particules (`electric_spark`, `firework` ou `happy_villager`). Les
destinataires d'un anneau, d'une orbite ou d'une traînée de missile sont résolus une
seule fois (`ParticleRequest::recipients`), puis chaque point est envoyé en `packet()`.
Un missile survit au départ de son tireur et peut encore toucher sa cible. Les
missiles laissent `flame` et `smoke` tous les deux ticks ; les pièges se signalent
tous les dix ticks (`item_slime` pour la banane, anneau `end_rod` de 2,5 blocs pour la
glace). Les karts en course émettent leur traînée tous les trois ticks, pour leurs
seuls spectateurs, selon leur état : `crit` (choc), `electric_spark` (bouclier),
`end_rod` (glace ou ralenti), `happy_villager` (super-recharge), `flame` (turbo ou
boost), `cloud` (vitesse > 0,25). Un kart immobile sans effet n'émet rien.

L'état complet — pièges, missiles, ondes (positions, âges, rayons, identifiant unique)
et emplacements de cristaux — est exposé par la ressource `Items`. Tout est retiré à la
fin de la manche, cristaux compris.

### Visuels et animation

Les effets prennent corps par des **entités display du moteur** (`src/displays.rs`),
en plus des particules. Chaque élément de décor animé est identifié par une clé
stable — source (kart, piège, missile ou onde), genre et numéro de pièce — et
correspond à **une seule entité** `block_display` ou `item_display`, créée à
l'apparition de la clé et retirée à sa disparition, jamais recréée entre-temps. Le
moteur se charge du reste : apparition chez les joueurs qui chargent le chunk,
retrait quand ils s'éloignent, métadonnées complètes pour un spectateur qui arrive
en cours d'animation.

Les huit genres reprennent les modèles de la référence, tous issus de `voidmc_data` :

- **Bonus tenu** : l'objet du bonus (`fire_charge`, `shield`, `yellow_dye`,
  `firework_rocket`, `ender_pearl`, `blue_ice`, `lightning_rod`, `nether_star`) flotte
  2,5 blocs au-dessus du kart, oscille et tourne lentement.
- **Réacteurs** (turbo ou boost) : deux flammes de verre orange et sea lantern à
  l'arrière du kart, dont la longueur pulse.
- **Bouclier** : huit facettes de verre cyan (blocs, pas des panneaux) en orbite
  ondulante autour du kart, apparition et extinction en fondu.
- **Super-recharge** : trois blocs d'émeraude en orbite inverse.
- **Débuff** (glace ou ralenti) : trois éclats de glace bleue qui tournent au ras du
  kart.
- **Pièges** : la banane est une hélice de béton jaune sur un pied d'or ; la glace,
  une nappe octogonale de glace bleue et quatre cristaux de verre bleu clair qui
  tournent lentement.
- **Missile** : fuselage de fer, ogive de béton rouge et tuyère sea lantern en
  rotation, orientés selon la piste.
- **Ondes** : douze segments en anneau (`emerald_block`, `orange_stained_glass`, ou
  sea lantern et verre cyan alternés) dont le rayon suit l'onde ; l'éclair est une
  colonne de cinq prismes en zigzag.

L'animation est **échantillonnée tous les deux ticks** (10 Hz), la cadence native de
l'interpolation client : les displays reçoivent `interpolation_ticks = 2` et
`teleport_ticks = 2`, luminosité maximale et portée de vue doublée. À chaque image,
le serveur ne réécrit que ce qui a changé — position de l'entité, transformation
(translation, échelle, rotation) ou modèle — et le moteur n'envoie que les entrées
de métadonnées modifiées, précédées du redémarrage de l'horloge d'interpolation.
Un décor immobile (une banane posée) ne génère aucun paquet. Les ticks impairs sont
ignorés en bloc. Quand la source d'un décor **saute** de plus de `displays::JUMP`
(6 blocs) entre deux images — retour au checkpoint par `/reset` —, ses entités sont
retirées et recréées à la nouvelle position au lieu de glisser à travers la carte en
interpolation. Les rotations sont des quaternions composés (lacet puis tangage)
pour rester continues au passage de ±180° ; les modèles de bloc, qui pivotent autour
de leur coin, sont recentrés sur le point demandé. Les fondus utilisent `smoothstep`
et une enveloppe qui atteint zéro deux ticks avant le retrait.

## Carte procédurale

Le **décor permanent** utilise plusieurs échelles de bruit pour ses vallées, lacs,
berges de sable et gravier, crêtes rocheuses et sommets enneigés. Des sapins de tailles
variées, des rochers moussus, des fougères et des fleurs complètent les versants.
Les décorations traversent les frontières de chunks. L'attente se fait en vol libre
au-dessus du décor naturel, sans construction de lobby.

Le **circuit de chaque manche** est une spline fermée traversant **10 à 14 points de
passage tirés aléatoirement** : distances au centre, angles, orientation, étirement
et altitudes changent. La route, large d'environ quatorze blocs, dispose d'un
revêtement gris texturé, d'un marquage central discontinu, de vibreurs cyan/orange et
blancs, de parapets vitrés, de supports de viaduc et de portiques éclairés. Les huit
checkpoints sont espacés selon la **distance parcourue sur la courbe**, commune au
pilotage et à la génération.

`VOID_DEMO_SEED` fixe le décor et la séquence des seeds de manches ; redémarrer avec
la même valeur reproduit cette séquence, et `/race [tours] <seed>` rejoue un circuit
précis quel que soit le décor. Tous les bits de la seed participent à la
génération. Le décor reste identique entre les manches.

Pour une présentation, une distance d'affichage client de 12 chunks permet de
profiter du paysage. Seuls les chunks traversés par le circuit sont mis à jour lors
de la construction ; le démontage suit l'ordre inverse et restaure exactement le
terrain initial, y compris les arbres dégagés pour la piste. Le profil `--release`
est recommandé.

## Organisation technique

- `src/terrain.rs` : paysage permanent, bruit de relief et décoration déterministe
  (`Alpine`, générateur `WorldGenerator`, biome taïga).
- `src/track.rs` : spline procédurale, index spatial par chunk, projection sur le
  tracé, distances de course et superposition des blocs du circuit (`Track`, `GATES`,
  `HALF_WIDTH`).
- `src/arena.rs` : générateur partagé avec le streaming (`Arena`, `WAIT_Y`) ;
  `Arena::replace` remplace les données ECS d'un chunk et le renvoie aux joueurs qui
  l'ont chargé via `WorldPlayers::broadcast_chunk`. Un circuit démonté ne réapparaît
  pas lors d'un chargement ultérieur.
- `src/kart.rs` : simulation pure, sans réseau — `Input`, `Kart` (grille, direction,
  accélération, traînée, glissières, `collide`, `crosses_gate`, `progress`),
  `PowerUp` et `Strike` (effets des bonus sur le véhicule). Les constantes numériques
  sont celles de la référence ; le yaw du modèle de minecart est décalé de +90°.
- `src/vehicle.rs` : le kart comme entité du moteur — `spawn` (`EntityBuilder`
  minecart + `Kart` + `Pilot` + `Passengers`), `Karts` (accès au kart d'un joueur via
  `Racer::kart`), l'observateur `input` (`PlayerInputEvent` → `Kart::input`,
  renvoi des passagers sur Sneak), le système `drive` (`Kart::drive` puis `collide`
  pendant la course) et `pose`, qui ne réécrit `Position`/`Rotation` du kart et la
  `Position` du pilote (`SEAT_HEIGHT` au-dessus) que si la simulation a bougé le kart.
- `src/items.rs` : arsenal arcade — la ressource `Items` (`Pickup`, `Trap`, `Missile`,
  `Burst`), le système `crystals` (End Crystals du moteur, apparition/disparition sur
  changement d'état seulement), `update` (activation des bonus, ramassage, pièges,
  guidage des missiles, vieillissement des ondes ; vide tout hors course) et
  `effects` (émission des particules via `Particles`, aux cadences de référence).
- `src/displays.rs` : les affichages — `Scene` (une entité display persistante par
  `Key`), le système `sync` (images tous les deux ticks, réécriture sur changement
  seulement, retrait hors course), les modèles par genre (`kart_frames`, pièges,
  missiles, ondes) et les mathématiques de rotation et de fondu (`rotation`,
  `rotate`, `smoothstep`, `envelope`).
- `src/travel.rs` : les déplacements des joueurs — `Travel` (`fly`, `to_lobby`,
  `everyone_to_lobby`, `board`) qui n'utilise que les primitives du moteur
  (`Teleport`, `PlayerAbilities`, `ServerControlledPosition`, `Passengers`), les
  marqueurs `Airborne` et `Transfer { Lobby | Boarding }`, l'observateur `arrived`
  (`PlayerTeleportEvent` : embarquement, délai dépassé, annulation) et `keep_flying`
  (`PlayerToggleFlyEvent`). L'état de la barrière vit dans le moteur ; aucun travail
  par tick pour un joueur qui n'est pas en transfert.
- `src/chat.rs` : palette et surfaces des messages — `Tone`, `Chat` (`say`,
  `say_all`, `say_others`, `podium`, `hud`, `flash`, `hud_free`) et le composant
  `Flash`.
- `src/audio.rs` : les sons — `Cue` (nom d'événement, volume, hauteur, catégorie) et
  `Audio` (`ui`, `ui_at`, `everyone`, `at`) au-dessus de `voidmc::Sounds`.
- `src/race.rs` : phases, commandes (`SeedArg` pour la seed de circuit), annonces,
  bossbars et HUD ; `Racer` relie le joueur à son kart et chronomètre ses tours
  (`lap_start`, `best_lap`) ; `circuit_seed` dérive la seed d'une manche tirée au
  hasard.
- `src/sidebar.rs` : le panneau latéral et la liste d'onglets — `board` (un `Sidebar`
  du moteur par joueur), `tab` (`TabList` partagée), le composant `Board` (poignées de
  widgets et dernière `View` affichée), la ressource `Standings` (classement partagé,
  versionné), les systèmes `standings`, `sync` et `tab_list`, et `clock_tenths`.
- `src/lib.rs` : `seed` (variable d'environnement ou tirage aléatoire non nul).
- `src/main.rs` : configuration par variables d'environnement et démarrage du serveur.

## Vérifications

```sh
cargo test -p voidmc-demo
```

Les tests couvrent la génération déterministe (paysage et arène), la diversité des
seeds, la continuité de 65 tracés, le parcours de trois tours sur plusieurs
géométries, les checkpoints ordonnés, la marche arrière, les rebonds, les bumps, les
boucliers, les bonus à usage unique et leurs effets, la préservation du paysage
pendant la construction et la restauration exacte au démontage. Les tests d'`App`
sans réseau vérifient l'inscription (un seul minecart par pilote, stationné, vide et
caché : aucun `SpawnEntity` tant que le pilote vole),
le renvoi unique des passagers par pression de Sneak une fois à bord, le déplacement
de l'entité et du pilote pendant la course (et l'absence de tout paquet ou écriture
pour un kart immobile), la disparition du kart au départ du joueur et le choc entre
deux karts, identique à la simulation pure. Pour les déplacements : le vol accordé à
l'arrivée (paquet `PlayerAbilities` 0x07), le retour de tous au point d'attente sur
`/race`, l'embarquement qui attend les chunks de la grille, le *Pong* au bon
identifiant puis la confirmation du bon identifiant avant de monter le pilote
(`SpawnEntity` du kart avant `SetPassengers`, une seule fois, vol retiré en 0x01), le
délai dépassé (pilote retiré, message, renvoi en vol, kart retiré chez tous, position
toujours synchronisée), la déconnexion ou l'annulation en plein transfert (kart caché,
pilote hors manche), le retour des karts cachés en fin de manche, le vol rendu quand le
client le coupe en l'air seulement, `/leave` qui renvoie en vol sans HUD en transfert et
`/join` qui ne renvoie rien, et un transfert vers le point d'attente qui recouvre un
embarquement inachevé et libère bien la position.
Pour l'arsenal : le cycle apparition / ramassage / réapparition des 24 cristaux
(entités, métadonnées End Crystal, paquets spawn/remove, aucun renvoi entre-temps,
retrait en fin de manche), le tirage des huit bonus, le guidage du missile sur le
pilote devant (géométrie de référence, bouclier, cible qui quitte, tireur qui quitte,
tir sans cible), deux karts sur un même cristal et le ramassage au tick de réapparition,
les ondes de choc et éclairs (portée, bouclier, spectateurs et pilotes arrivés
ignorés, vieillissement), les pièges (délai du poseur, glace persistante, banane
consommée, expiration) et les particules capturées sur le fil aux ticks attendus,
avec les types, quantités, offsets et audiences de référence. Pour les affichages :
un bonus tenu produit exactement une entité `item_display` qui persiste sur dix ticks,
ne reçoit une métadonnée (redémarrage d'interpolation, translation, rotation) qu'aux
ticks pairs, change d'objet sans être recréée, suit un kart en mouvement par un
déplacement relatif aux seuls ticks pairs, est recréée plutôt que glissée quand le
kart revient à son checkpoint, disparaît chez tous les spectateurs quand le bonus est
utilisé ou que le pilote se déconnecte ; une banane posée n'émet plus rien une fois stabilisée et
un joueur qui arrive ensuite reçoit ses quatre blocs avec leurs métadonnées complètes
par le moteur ; les huit genres ont une géométrie bornée (translations, échelles,
quaternions unitaires) sur 250 ticks sans aucun nouveau spawn et la scène se vide
d'elle-même ; les fondus et le recentrage des blocs sont vérifiés à l'unité, et
les octets d'une image de métadonnées est comparé à la disposition Paper 26.1.2
(index, sérialiseurs `Int`/`Vector3`/`Quaternion`, terminateur). Pour l'interface :
chaque message part avec la couleur de sa catégorie et sur la bonne surface (chat ou
barre d'action), un flash tient huit périodes de HUD avant que le HUD ne reprenne, le
record n'est annoncé qu'en battant le meilleur temps d'un circuit rejoué sur la même
seed (jamais à temps égal ni plus lent), un choc sur la ligne d'arrivée ne sonne qu'une
fois, le son de portail est placé au point d'attente, le compte à rebours
produit quatre bips (trois graves, un aigu) et un départ, le tour bouclé un seul son,
un choc un son par kart toutes les dix ticks exactement, chaque `Cue` résout un
`sound_event` du registre, et les octets d'un bip (`SoundEffect` : holder, catégorie,
position en huitièmes, volume, hauteur, seed) suivent Paper 26.1.2 ; les rafales
d'impact partent au tick de l'événement et jamais au suivant ; la seed est reprise
telle quelle depuis l'environnement ou tirée non nulle et différente à chaque appel.
Pour le panneau latéral : un panneau et l'en-tête d'onglets créés au premier tick après
l'arrivée avec les bonnes lignes, puis aucun paquet sur quarante ticks calmes ; les
trois dispositions au fil d'une manche complète (construction en pourcentage, jauge de
chargement pleine, classement au départ, arrivée, démontage, résultats avec soi en
jaune) sans que la bossbar ne rapporte plus la construction ; le chrono qui coûte une
ligne par seconde et rien entre ; un classement réécrit une seule fois quand deux
karts se dépassent et jamais quand ils roulent sans changer d'ordre ; le meilleur tour
fixé au franchissement de la ligne, conservé si le tour suivant est plus lent,
amélioré sinon, et effacé à la manche suivante avec le pied d'onglets qui passe à la
nouvelle seed ; le spectateur qui garde un panneau et la déconnexion qui retire
l'objectif.
