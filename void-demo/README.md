# Alpine Rush — démo Void

Mini-jeu de course arcade en minecart, **solo ou jusqu'à huit joueurs**, pour
**Minecraft Java 26.1.2**, sans mod ni resource pack. Le véhicule se pilote librement
sur une chaussée : ses déplacements sont simulés par le serveur.

> **État du portage.** Cette version couvre le socle (D0) du portage de la démo sur les
> API actuelles du moteur — génération du monde, géométrie du circuit et simulation
> pure des karts —, la machine à états de la course (D1) : phases, commandes,
> annonces et bossbars, les karts en tant qu'entités du moteur (D2) : minecart
> monté, pilotage au clavier et déplacement piloté par le serveur, l'arsenal
> arcade (D3) : cristaux de bonus, pièges, missiles, ondes et particules, et les
> affichages animés (D4) : bonus tenu, réacteurs, bouclier, recharge, débuffs, pièges,
> missiles et ondes en entités *display* persistantes. Le reste — barrière de
> téléportation — est **en cours de portage** et arrive dans l'unité suivante.

## Lancer

Depuis la racine du dépôt :

```sh
cargo run --release -p voidmc-demo
```

La démo écoute par défaut sur toutes les interfaces IPv4 (`0.0.0.0:25565`).
Se connecter à `127.0.0.1:25565` depuis la machine hôte, ou à son adresse IP depuis
une autre machine. Le joueur apparaît à **Y = 110**, au-dessus de la vallée, assis
dans son minecart d'attente ; le vol libre des spectateurs revient avec l'unité
travel (D5). Aucune plateforme ne gêne la vue du paysage.

Pour utiliser une autre carte :

```sh
VOID_DEMO_SEED=2026 cargo run --release -p voidmc-demo
```

`VOID_DEMO_ADDRESS` permet de modifier l'adresse et le port d'écoute. La seed est un
entier non signé sur 64 bits ; sa valeur par défaut est `42`. Le serveur tourne
à 20 ticks/s. `RUST_LOG` règle la verbosité des logs.

## Jouer

Chaque pilote inscrit est assis dans **un minecart** qui lui appartient (`src/vehicle.rs`).
Le minecart est une entité du moteur : le serveur le fait apparaître chez les joueurs
qui ont chargé son chunk, le retire quand ils s'éloignent ou se déconnectent, et
transmet ses déplacements. Le pilote en est le passager. Sur une pression de Sneak,
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

- `/race [tours]` accepte **1 à 20 tours** (3 par défaut). Le nombre choisi vaut pour
  tous les pilotes de la manche ; une commande invalide ne lance pas de course, et une
  manche en cours ne peut pas être relancée.
- Chaque manche construit un **nouveau tracé** dans la vallée, un chunk tous les deux
  ticks, avec une annonce à 25/50/75 %. La grille est placée dès que le circuit est
  prêt ; le chargement des chunks chez chaque pilote et la barrière de téléportation
  reviennent avec l'unité travel (D5), le compte à rebours de cinq secondes démarre
  donc immédiatement.
- Le nombre de tours choisi, avec huit checkpoints par tour, à franchir dans l'ordre.
  Le chat annonce chaque nouveau tour au pilote concerné, les arrivées, puis le podium.
  La limite de temps est de dix minutes jusqu'à trois tours, puis de 200 secondes par
  tour ; le chat prévient à 60, 30 et 10 secondes de la fin.
- La **barre d'action** des pilotes affiche la phase, puis tour, checkpoint, temps,
  pénalités, vitesse et bonus pendant la course. La **bossbar bleue** de chaque pilote
  affiche sa réserve de boost ; une bossbar partagée suit la construction, le compte
  à rebours et le temps restant.
- `/reset` ramène au dernier checkpoint avec trois secondes de pénalité ; le minecart
  y est téléporté par le moteur.
- `/scores` affiche les temps de la manche et les meilleurs temps du circuit actuel
  pour le même nombre de tours. Les records sont en mémoire, par seed de circuit,
  nombre de tours et nom de joueur, et disparaissent au redémarrage.
- `/leave` passe en spectateur ; `/join` inscrit pour le prochain départ. Chaque joueur
  qui arrive est inscrit automatiquement (huit pilotes au maximum). Une arrivée ou un
  retour pendant une manche attend la suivante ; un pilote qui se déconnecte pendant
  la course abandonne la manche.
- Lorsque tous les participants ont terminé ou quitté, le circuit se démonte dans
  l'ordre inverse et le terrain initial est restauré ; `/race` redevient disponible
  à la fin du démontage.

### Power-ups et effets

Dès que le circuit est prêt, **24 End Crystals** flottent au-dessus de la piste
(`src/items.rs`) : trois par checkpoint, à gauche, au centre et à droite, à mi-chemin
du checkpoint suivant. Chaque cristal est une entité du moteur (`EntityBuilder` +
`EndCrystal::floating()`, sans socle ni rayon) que le serveur fait apparaître chez les
joueurs qui ont chargé son chunk. Un pilote qui passe à moins de 1,6 bloc d'un
cristal, sans bonus en main, ramasse un des huit bonus (tirage déterministe à partir de
la seed du circuit, du tick, de l'emplacement et du kart) ; le cristal disparaît et
réapparaît **huit secondes** plus tard. Quatre particules `end_rod` orbitent autour de
chaque cristal présent, toutes les dix ticks, pour ses seuls spectateurs.

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
ignorés en bloc. Les rotations sont des quaternions composés (lacet puis tangage)
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
la même valeur reproduit cette séquence. Tous les bits de la seed participent à la
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
- `src/race.rs` : phases, commandes, annonces, bossbars et HUD ; `Racer` relie le
  joueur à son kart.
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
sans réseau vérifient l'inscription (un seul minecart par pilote, passager compris),
le renvoi unique des passagers par pression de Sneak, le déplacement de l'entité et
du pilote pendant la course (et l'absence de tout paquet ou écriture pour un kart
immobile), la disparition du kart
au départ du joueur et le choc entre deux karts, identique à la simulation pure.
Pour l'arsenal : le cycle apparition / ramassage / réapparition des 24 cristaux
(entités, métadonnées End Crystal, paquets spawn/remove, aucun renvoi entre-temps,
retrait en fin de manche), le tirage des huit bonus, le guidage du missile sur le
pilote devant (géométrie de référence, bouclier, cible qui quitte, tir sans cible),
les ondes de choc et éclairs (portée, bouclier, spectateurs et pilotes arrivés
ignorés, vieillissement), les pièges (délai du poseur, glace persistante, banane
consommée, expiration) et les particules capturées sur le fil aux ticks attendus,
avec les types, quantités, offsets et audiences de référence. Pour les affichages :
un bonus tenu produit exactement une entité `item_display` qui persiste sur dix ticks,
ne reçoit une métadonnée (redémarrage d'interpolation, translation, rotation) qu'aux
ticks pairs, change d'objet sans être recréée et disparaît chez tous les spectateurs
quand le bonus est utilisé ; une banane posée n'émet plus rien une fois stabilisée et
un joueur qui arrive ensuite reçoit ses quatre blocs avec leurs métadonnées complètes
par le moteur ; les huit genres ont une géométrie bornée (translations, échelles,
quaternions unitaires) sur 250 ticks sans aucun nouveau spawn et la scène se vide
d'elle-même ; les fondus et le recentrage des blocs sont vérifiés à l'unité, et
les octets d'une image de métadonnées est comparé à la disposition Paper 26.1.2
(index, sérialiseurs `Int`/`Vector3`/`Quaternion`, terminateur).
