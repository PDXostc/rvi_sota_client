use std::sync::mpsc::{Sender , Receiver};
use std::marker::PhantomData;

use http_client::HttpClient;
use ota_plus::{get_package_updates, download_package_update, post_packages};
use datatype::{Event, Command, Config, AccessToken, UpdateState, Package, Error, UpdateRequestId};

pub struct Interpreter<'a, C: HttpClient> {
    client_type: PhantomData<C>,
    config: &'a Config,
    token: AccessToken,
    // Commands mpsc, events spmc
    commands_rx: Receiver<Command>,
    events_tx: Sender<Event>
}

impl<'a, C: HttpClient> Interpreter<'a, C> {
    pub fn new(config: &'a Config, token: AccessToken, commands_rx: Receiver<Command>, events_tx: Sender<Event>) -> Interpreter<'a, C> {
        Interpreter { client_type: PhantomData, config: config, token: token, commands_rx: commands_rx, events_tx: events_tx }
    }

    pub fn start(&self) {
        loop {
            self.interpret(self.commands_rx.recv().unwrap());
        }
    }

    pub fn interpret(&self, command: Command) {
        match command {
            Command::GetPendingUpdates => self.get_pending_updates(),
            Command::PostInstalledPackages => self.post_installed_packages(),
            Command::AcceptUpdate(ref id) => self.accept_update(id),
            Command::ListInstalledPackages => self.list_installed_packages()
        }
    }

    fn publish(&self, event: Event) {
        let _ = self.events_tx.send(event);
    }

    fn get_installed_packages(&self) -> Result<Vec<Package>, Error> {
        let pkg_manager = self.config.ota.package_manager.build();
        pkg_manager.installed_packages(&self.config.ota)
    }

    fn get_pending_updates(&self) {
        debug!("Fetching package updates...");
        let response: Event = match get_package_updates::<C>(&self.token, &self.config.ota) {
            Ok(updates) => {
                let update_events: Vec<Event> = updates.iter().map(move |id| Event::NewUpdateAvailable(id.clone())).collect();
                info!("New package updates available: {:?}", update_events);
                Event::Batch(update_events)
            },
            Err(e) => {
                Event::Error(format!("{}", e))
            }
        };
        self.publish(response);
    }

    fn post_installed_packages(&self) {
        let _ = self.get_installed_packages().and_then(|pkgs| {
            debug!("Found installed packages in the system: {:?}", pkgs);
            post_packages::<C>(&self.token, &self.config.ota, &pkgs)
        }).map(|_| {
            info!("Posted installed packages to the server.");
        }).map_err(|e| {
            error!("Error fetching/posting installed packages: {:?}.", e);
        });
    }

    fn accept_update(&self, id: &UpdateRequestId) {
        info!("Accepting update {}...", id);
        self.publish(Event::UpdateStateChanged(id.clone(), UpdateState::Accepted));
        let _ = download_package_update::<C>(&self.token, &self.config.ota, id)
            .and_then(|path| {
                info!("Downloaded at {:?}. Installing...", path);
                self.publish(Event::UpdateStateChanged(id.clone(), UpdateState::Installing));
                let pkg_manager = self.config.ota.package_manager.build();
                pkg_manager.install_package(&self.config.ota, path.to_str().unwrap())
            }).map(|_| {
                info!("Update installed successfully.");
                self.publish(Event::UpdateStateChanged(id.clone(), UpdateState::Installed));
                self.interpret(Command::PostInstalledPackages);
            }).map(|_| {
                debug!("Notified the server of the new state.");
            }).map_err(|e| {
                error!("Error updating. State: {:?}", e);
                self.publish(Event::UpdateErrored(id.clone(), format!("{:?}", e)));
            });
    }

    fn list_installed_packages(&self) {
        let _ = self.get_installed_packages().and_then(|pkgs| {
            self.publish(Event::FoundInstalledPackages(pkgs.clone()));
            Ok(())
        });
    }
}
